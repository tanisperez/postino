//! Integration tests for [`postino_load::LoadRun`], against a local `tiny_http` server and the
//! real `QuickJsEngine`.
//!
//! No test here ever touches the internet: every server is bound to `127.0.0.1` on an
//! OS-assigned port by `postino_http::test_support::TestServer`. Every test finishes in well
//! under 3 seconds.
// `expect()` is the normal way to fail a test with a clear message; allowed crate-wide since
// this whole file is test code.
#![allow(clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use postino_core::{Environment, KeyValue, Method, Request};
use postino_http::SendOptions;
use postino_http::test_support::{CapturedRequest, StubResponse, TestServer};
use postino_load::{LoadConfig, LoadRun, LoadTarget, StatusKey};
use postino_runner::{Runner, SessionEnv};
use postino_script::QuickJsEngine;
use pretty_assertions::assert_eq;

/// A bare, enabled [`Runner`] backed by the real `QuickJsEngine`, sending with default
/// [`SendOptions`].
fn runner() -> Arc<Runner> {
    Arc::new(Runner::new(Arc::new(QuickJsEngine), SendOptions::default()))
}

/// A bare GET request against `url`, no headers, no body, no scripts.
fn get(url: impl Into<String>) -> Request {
    Request {
        method: Method::Get,
        url: url.into(),
        ..Request::default()
    }
}

#[test]
fn a_short_run_produces_samples_for_every_target_with_a_200_status_count() {
    let server = TestServer::start();
    let handle = server.serve(|_| StubResponse::new(200));

    let config = LoadConfig {
        targets: vec![
            LoadTarget::new("a", get(format!("{}/a", server.url()))),
            LoadTarget::new("b", get(format!("{}/b", server.url()))),
        ],
        vus: 4,
        duration: Duration::from_secs(1),
        ramp_up: Duration::ZERO,
        think_time: Duration::ZERO,
        stop_on_error_rate: None,
    };

    let run = LoadRun::start(config, Environment::default(), SessionEnv::new(), runner());
    let summary = run.join();

    drop(server);
    handle.join().expect("test server thread panicked");

    assert!(!summary.stopped_early);
    assert!(summary.snapshot.total > 0, "expected at least one sample");
    assert_eq!(summary.snapshot.per_target.len(), 2);
    assert!(
        summary.snapshot.per_target[0].count > 0,
        "target 'a' got no samples"
    );
    assert!(
        summary.snapshot.per_target[1].count > 0,
        "target 'b' got no samples"
    );
    assert_eq!(
        summary.snapshot.status_counts,
        vec![(StatusKey::Code(200), summary.snapshot.total)]
    );
    assert!((summary.snapshot.error_rate - 0.0).abs() < f64::EPSILON);
}

#[test]
fn ramp_up_starts_fewer_virtual_users_early_than_late() {
    let server = TestServer::start();
    let handle = server.serve(|_| StubResponse::new(200));

    let config = LoadConfig {
        targets: vec![LoadTarget::new("a", get(server.url()))],
        vus: 10,
        duration: Duration::from_millis(600),
        ramp_up: Duration::from_millis(400),
        think_time: Duration::from_millis(5),
        stop_on_error_rate: None,
    };

    let run = LoadRun::start(config, Environment::default(), SessionEnv::new(), runner());

    std::thread::sleep(Duration::from_millis(60));
    let early_active = run.snapshot().active_vus;

    std::thread::sleep(Duration::from_millis(450));
    let late_active = run.snapshot().active_vus;

    run.stop();
    let _ = run.join();
    drop(server);
    handle.join().expect("test server thread panicked");

    assert!(
        early_active < late_active,
        "expected fewer virtual users early ({early_active}) than late ({late_active})"
    );
    assert!(
        late_active >= 8,
        "expected ramp-up to have nearly finished by now, got {late_active}"
    );
}

#[test]
fn stop_ends_the_run_promptly() {
    let server = TestServer::start();
    let handle = server.serve(|_| StubResponse::new(200));

    let config = LoadConfig {
        targets: vec![LoadTarget::new("a", get(server.url()))],
        vus: 2,
        duration: Duration::from_secs(5),
        ramp_up: Duration::ZERO,
        think_time: Duration::ZERO,
        stop_on_error_rate: None,
    };

    let run = LoadRun::start(config, Environment::default(), SessionEnv::new(), runner());
    std::thread::sleep(Duration::from_millis(50));

    let started_stopping = Instant::now();
    run.stop();
    let summary = run.join();
    let stop_latency = started_stopping.elapsed();

    drop(server);
    handle.join().expect("test server thread panicked");

    assert!(summary.stopped_early);
    assert!(
        stop_latency < Duration::from_millis(500),
        "stop() took {stop_latency:?} to take effect"
    );
}

#[test]
fn a_server_returning_500_with_a_stop_on_error_rate_stops_the_run_early() {
    let server = TestServer::start();
    let handle = server.serve(|_| StubResponse::new(500));

    let config = LoadConfig {
        targets: vec![LoadTarget::new("a", get(server.url()))],
        vus: 10,
        // A generous nominal duration: if the error-rate stop condition ever regressed, this
        // test would still finish within the 3 second budget instead of hanging, just less
        // conclusively.
        duration: Duration::from_secs(2),
        ramp_up: Duration::ZERO,
        think_time: Duration::ZERO,
        stop_on_error_rate: Some(0.1),
    };

    let started = Instant::now();
    let run = LoadRun::start(config, Environment::default(), SessionEnv::new(), runner());
    let summary = run.join();
    let elapsed = started.elapsed();

    drop(server);
    handle.join().expect("test server thread panicked");

    assert!(summary.stopped_early);
    assert!((summary.snapshot.error_rate - 1.0).abs() < f64::EPSILON);
    assert!(summary.snapshot.total >= 50);
    assert!(
        elapsed < Duration::from_secs(1),
        "expected the error rate threshold to stop the run well before its 2 second duration, \
         took {elapsed:?}"
    );
}

#[test]
fn a_post_script_env_change_flows_to_the_next_target_of_the_same_virtual_user() {
    let server = TestServer::start();
    let request_count = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel();
    let counter = Arc::clone(&request_count);
    let handle = server.serve(move |captured: CapturedRequest| {
        counter.fetch_add(1, Ordering::SeqCst);
        let _ = tx.send(captured.clone());
        if captured.url == "/login" {
            StubResponse::new(200).with_body(r#"{"token":"abc123"}"#)
        } else {
            StubResponse::new(200)
        }
    });

    let login = Request {
        method: Method::Post,
        url: format!("{}/login", server.url()),
        post_script: r#"
            var body = res.json();
            env.set("token", body.token);
        "#
        .to_string(),
        ..Request::default()
    };
    let me = Request {
        method: Method::Get,
        url: format!("{}/me", server.url()),
        headers: vec![KeyValue::new("Authorization", "Bearer {{token}}")],
        ..Request::default()
    };

    let config = LoadConfig {
        targets: vec![LoadTarget::new("login", login), LoadTarget::new("me", me)],
        vus: 2,
        duration: Duration::from_millis(300),
        ramp_up: Duration::ZERO,
        think_time: Duration::from_millis(20),
        stop_on_error_rate: None,
    };

    let run = LoadRun::start(config, Environment::default(), SessionEnv::new(), runner());
    let summary = run.join();

    drop(server);
    handle.join().expect("test server thread panicked");

    assert!(summary.snapshot.total > 0);

    let mut me_requests_seen = 0;
    while let Ok(captured) = rx.recv_timeout(Duration::from_millis(100)) {
        if captured.url == "/me" {
            me_requests_seen += 1;
            let authorization = captured
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                .map(|(_, value)| value.as_str());
            assert_eq!(
                authorization,
                Some("Bearer abc123"),
                "a 'me' request did not see the token its own virtual user's login set"
            );
        }
    }
    assert!(me_requests_seen > 0, "expected at least one 'me' request");
}
