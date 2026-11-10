//! A request whose auth is an OAuth 2.0 profile (#225): the runner gets the token before the pre
//! script and sends it the way the profile says.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::mpsc;

use postino_core::{
    Auth, Environment, KeyValue, Method, OAuthGrant, OAuthProfile, Request, TokenPlacement,
};
use postino_http::SendOptions;
use postino_http::test_support::{StubResponse, TestServer};
use postino_oauth::{MemoryStore, NoSignIn, OAuthError, TokenEvent};
use postino_runner::{FailedStage, OAuthConfig, Runner, SessionEnv};
use postino_script::QuickJsEngine;
use pretty_assertions::assert_eq;

fn runner(profile: OAuthProfile) -> Runner {
    Runner::new(Arc::new(QuickJsEngine), SendOptions::default()).with_oauth(OAuthConfig {
        workspace: "ws-000000".into(),
        profiles: Arc::new(vec![profile]),
        store: Arc::new(MemoryStore::default()),
        sign_in: Arc::new(NoSignIn),
    })
}

fn request(url: String) -> Request {
    Request {
        method: Method::Get,
        url,
        auth: Auth::OAuth2 {
            profile: "machine".into(),
            scope: String::new(),
        },
        pre_script: "console.log(req.headers.length)".into(),
        ..Request::default()
    }
}

fn environment() -> Environment {
    Environment {
        name: "dev".into(),
        variables: vec![KeyValue::new("secret", "s")],
    }
}

#[test]
fn the_token_is_obtained_once_and_sent_as_a_header_or_a_query_parameter() {
    let server = TestServer::start();
    let (tx, rx) = mpsc::channel();
    let handle = server.serve(move |captured| {
        let answer = if captured.url == "/token" {
            StubResponse::new(200)
                .with_body(r#"{"access_token":"tok","token_type":"Bearer","expires_in":3600}"#)
        } else {
            StubResponse::new(200).with_body("ok")
        };
        let _ = tx.send(captured);
        answer
    });
    let mut profile = OAuthProfile::new("machine", OAuthGrant::ClientCredentials);
    profile.token_url = format!("{}/token", server.url());
    profile.client_id = "id".into();
    profile.client_secret = "{{secret}}".into();
    let first_runner = runner(profile.clone());
    let mut session = SessionEnv::default();

    let first = first_runner.run(
        &request(format!("{}/api", server.url())),
        &environment(),
        &mut session,
    );
    assert!(first.failed_stage.is_none(), "{:?}", first.failed_stage);
    assert_eq!(first.oauth.as_ref().unwrap().event, Some(TokenEvent::New));
    let token_request = rx.recv().unwrap();
    assert_eq!(token_request.url, "/token");
    let api = rx.recv().unwrap();
    assert!(
        api.headers.iter().any(
            |(name, value)| name.eq_ignore_ascii_case("authorization") && value == "Bearer tok"
        ),
        "{:?}",
        api.headers
    );

    let second = first_runner.run(
        &request(format!("{}/api", server.url())),
        &environment(),
        &mut session,
    );
    assert_eq!(
        second.oauth.as_ref().unwrap().event,
        Some(TokenEvent::Reused)
    );
    assert_eq!(rx.recv().unwrap().url, "/api");

    profile.token_placement = TokenPlacement::Query;
    let query_runner = runner(profile);
    query_runner.run(
        &request(format!("{}/api", server.url())),
        &environment(),
        &mut session,
    );
    let _token = rx.recv().unwrap();
    assert_eq!(rx.recv().unwrap().url, "/api?access_token=tok");

    drop(server);
    handle.join().unwrap();
}

#[test]
fn an_unknown_profile_stops_the_run_before_the_pre_script() {
    let runner = runner(OAuthProfile::new("other", OAuthGrant::ClientCredentials));
    let mut session = SessionEnv::default();
    let result = runner.run(
        &request("http://127.0.0.1:9/".into()),
        &environment(),
        &mut session,
    );
    assert!(matches!(result.failed_stage, Some(FailedStage::Auth)));
    assert!(
        matches!(result.oauth_error(), Some(OAuthError::UnknownProfile(name)) if name == "machine")
    );
    assert!(result.console.is_empty());
    assert!(result.resolved_request.is_none());
}
