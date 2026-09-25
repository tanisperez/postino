//! Integration tests for `postino_http::send`, against a local `tiny_http` server.
//!
//! No test here ever touches the internet: every server is bound to `127.0.0.1` on an
//! OS-assigned port by `postino_http::test_support::TestServer`.
// `expect()` is the normal way to fail a test with a clear message; allowed crate-wide since
// this whole file is test code.
#![allow(clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use postino_core::{Method, ResolvedBody, ResolvedField, ResolvedRequest};
use postino_http::test_support::{CapturedRequest, StubResponse, TestServer};
use postino_http::{HttpError, SendOptions};

/// A bare request for `method` against `url`, no headers, no body.
fn request_to(method: Method, url: impl Into<String>) -> ResolvedRequest {
    ResolvedRequest {
        method,
        url: url.into(),
        ..ResolvedRequest::default()
    }
}

/// The outcome of a [`run`] call: the request the server captured (`None` if it never received
/// one, for example in the timeout test) and the result `postino_http::send` returned.
struct TestRun {
    captured: Option<CapturedRequest>,
    result: Result<postino_core::Response, HttpError>,
}

/// Starts a server, builds a request against it with `build_request`, has the server answer
/// every request with `respond_with`, and sends the request through `postino_http::send`.
///
/// Only the first captured request is kept (some tests, like the redirect ones, cause more than
/// one). The server is shut down and its thread joined before returning, so a panic inside
/// `respond_with` (for example a failed assertion) fails the test.
fn run<B, F>(build_request: B, options: &SendOptions, respond_with: F) -> TestRun
where
    B: FnOnce(&str) -> ResolvedRequest,
    F: Fn(CapturedRequest) -> StubResponse + Send + 'static,
{
    let server = TestServer::start();
    let request = build_request(&server.url());

    let (tx, rx) = mpsc::channel();
    let handle = server.serve(move |captured| {
        let response = respond_with(captured.clone());
        // The receiver may already have what it needs in tests that trigger more than one
        // request (redirects); a dropped receiver here is not a bug in the server.
        let _ = tx.send(captured);
        response
    });

    let result = postino_http::send(&request, options);
    let captured = rx.recv_timeout(Duration::from_secs(1)).ok();

    drop(server);
    handle.join().expect("test server thread panicked");

    TestRun { captured, result }
}

#[test]
fn sends_every_standard_method() {
    let methods = [
        (Method::Get, "GET"),
        (Method::Post, "POST"),
        (Method::Put, "PUT"),
        (Method::Patch, "PATCH"),
        (Method::Delete, "DELETE"),
        (Method::Head, "HEAD"),
        (Method::Options, "OPTIONS"),
        (Method::Custom("PURGE".to_string()), "PURGE"),
    ];

    for (method, expected) in methods {
        let run = run(
            move |url| request_to(method, url),
            &SendOptions::default(),
            |_| StubResponse::new(200),
        );

        let captured = run
            .captured
            .expect("server should have received the request");
        assert_eq!(captured.method, expected);
        let response = run.result.expect("request should succeed");
        assert_eq!(response.status, 200);
    }
}

#[test]
fn sends_request_headers() {
    let run = run(
        |url| {
            let mut request = request_to(Method::Get, url);
            request.headers.push(ResolvedField {
                name: "X-Test".to_string(),
                value: "hello".to_string(),
            });
            request
        },
        &SendOptions::default(),
        |_| StubResponse::new(200),
    );

    let captured = run
        .captured
        .expect("server should have received the request");
    assert!(
        captured
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("x-test") && value == "hello"),
        "expected an X-Test: hello header, got {:?}",
        captured.headers
    );
    run.result.expect("request should succeed");
}

#[test]
fn receives_response_headers() {
    let run = run(
        |url| request_to(Method::Get, url),
        &SendOptions::default(),
        |_| StubResponse::new(200).with_header("X-Reply", "world"),
    );

    let response = run.result.expect("request should succeed");
    assert!(
        response
            .headers
            .iter()
            .any(|field| field.name.eq_ignore_ascii_case("x-reply") && field.value == "world"),
        "expected an X-Reply: world header, got {:?}",
        response.headers
    );
}

#[test]
fn sends_json_body() {
    let run = run(
        |url| {
            let mut request = request_to(Method::Post, url);
            request.headers.push(ResolvedField {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            });
            request.body = ResolvedBody::Bytes(br#"{"name":"postino"}"#.to_vec());
            request
        },
        &SendOptions::default(),
        |_| StubResponse::new(200),
    );

    let captured = run
        .captured
        .expect("server should have received the request");
    assert_eq!(captured.body, br#"{"name":"postino"}"#);
    assert!(
        captured
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("content-type")
                && value == "application/json")
    );
    run.result.expect("request should succeed");
}

#[test]
fn sends_text_body() {
    let run = run(
        |url| {
            let mut request = request_to(Method::Post, url);
            request.headers.push(ResolvedField {
                name: "Content-Type".to_string(),
                value: "text/plain".to_string(),
            });
            request.body = ResolvedBody::Bytes(b"hello, postino".to_vec());
            request
        },
        &SendOptions::default(),
        |_| StubResponse::new(200),
    );

    let captured = run
        .captured
        .expect("server should have received the request");
    assert_eq!(captured.body, b"hello, postino");
    run.result.expect("request should succeed");
}

#[test]
fn sends_form_body() {
    let run = run(
        |url| {
            let mut request = request_to(Method::Post, url);
            request.headers.push(ResolvedField {
                name: "Content-Type".to_string(),
                value: "application/x-www-form-urlencoded".to_string(),
            });
            request.body = ResolvedBody::Bytes(b"a=1&b=2".to_vec());
            request
        },
        &SendOptions::default(),
        |_| StubResponse::new(200),
    );

    let captured = run
        .captured
        .expect("server should have received the request");
    assert_eq!(captured.body, b"a=1&b=2");
    run.result.expect("request should succeed");
}

#[test]
fn returns_404_as_a_normal_response() {
    let run = run(
        |url| request_to(Method::Get, url),
        &SendOptions::default(),
        |_| StubResponse::new(404).with_body("missing"),
    );

    let response = run.result.expect("a 404 is a response, not an error");
    assert_eq!(response.status, 404);
    assert_eq!(response.body, b"missing");
}

#[test]
fn returns_500_as_a_normal_response() {
    let run = run(
        |url| request_to(Method::Get, url),
        &SendOptions::default(),
        |_| StubResponse::new(500).with_body("oops"),
    );

    let response = run.result.expect("a 500 is a response, not an error");
    assert_eq!(response.status, 500);
    assert_eq!(response.body, b"oops");
}

#[test]
fn follows_redirects_by_default() {
    let request_count = Arc::new(AtomicUsize::new(0));
    let count_for_handler = Arc::clone(&request_count);

    let run = run(
        |url| request_to(Method::Get, url),
        &SendOptions::default(),
        move |_| {
            let seen = count_for_handler.fetch_add(1, Ordering::SeqCst);
            if seen == 0 {
                StubResponse::new(302).with_header("Location", "/target")
            } else {
                StubResponse::new(200).with_body("landed")
            }
        },
    );

    let response = run
        .result
        .expect("request should succeed after following the redirect");
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"landed");
    assert_eq!(request_count.load(Ordering::SeqCst), 2);
}

#[test]
fn does_not_follow_redirects_when_disabled() {
    let options = SendOptions {
        follow_redirects: false,
        ..SendOptions::default()
    };

    let run = run(
        |url| request_to(Method::Get, url),
        &options,
        |_| StubResponse::new(302).with_header("Location", "/target"),
    );

    let response = run
        .result
        .expect("request should succeed, redirect included");
    assert_eq!(response.status, 302);
    assert!(
        response
            .headers
            .iter()
            .any(|field| field.name.eq_ignore_ascii_case("location"))
    );
}

#[test]
fn times_out_when_the_server_is_too_slow() {
    let options = SendOptions {
        timeout: Duration::from_millis(100),
        ..SendOptions::default()
    };

    let run = run(
        |url| request_to(Method::Get, url),
        &options,
        |_| {
            std::thread::sleep(Duration::from_millis(600));
            StubResponse::new(200)
        },
    );

    match run.result {
        Err(HttpError::Timeout) => {}
        other => panic!("expected a timeout error, got {other:?}"),
    }
}
