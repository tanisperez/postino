//! [`ensure_token`] end to end against a local token endpoint.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use postino_core::{KeyValue, OAuthGrant, OAuthProfile, VarScope};
use postino_http::SendOptions;
use postino_http::test_support::{CapturedRequest, StubResponse, TestServer};
use postino_oauth::{
    Force, MemoryStore, NoSignIn, OAuthEnv, OAuthError, OAuthNote, OAuthStep, SignInHandler,
    SignInPoll, SignInPrompt, TokenEvent, TokenSet, TokenStore, ensure_token, query_params,
    token_key,
};
use pretty_assertions::assert_eq;

fn body(request: &CapturedRequest) -> String {
    String::from_utf8_lossy(&request.body).into_owned()
}

fn profile(server: &TestServer, grant: OAuthGrant) -> OAuthProfile {
    let mut profile = OAuthProfile::new("acme", grant);
    profile.token_url = format!("{}/oauth/token", server.url());
    profile.auth_url = format!("{}/authorize", server.url());
    profile.client_id = "{{clientId}}".into();
    profile.client_secret = "{{clientSecret}}".into();
    profile.scope = "read".into();
    profile.username = "ana".into();
    profile.password = "pw".into();
    profile
}

fn environment() -> Vec<KeyValue> {
    vec![
        KeyValue::new("clientId", "postino"),
        KeyValue::new("clientSecret", "s3cr3t"),
    ]
}

fn token_json(access: &str, refresh: Option<&str>) -> String {
    match refresh {
        Some(refresh) => format!(
            r#"{{"access_token":"{access}","token_type":"Bearer","expires_in":3600,"refresh_token":"{refresh}"}}"#
        ),
        None => format!(r#"{{"access_token":"{access}","token_type":"Bearer","expires_in":3600}}"#),
    }
}

fn run(
    store: &dyn TokenStore,
    profile: &OAuthProfile,
    sign_in: &dyn SignInHandler,
    force: Force,
) -> postino_oauth::OAuthRun {
    let environment = environment();
    let vars = VarScope {
        request_vars: &[],
        session_env: &[],
        environment: &environment,
    };
    let options = SendOptions::default();
    let env = OAuthEnv {
        workspace: "ws-123456",
        environment: "dev",
        store,
        options: &options,
        sign_in,
    };
    ensure_token(&env, profile, "", &vars, force)
}

fn expire(store: &dyn TokenStore, profile: &OAuthProfile) {
    let key = token_key("ws-123456", "dev", profile, "");
    let mut token: TokenSet = store.get(&key).unwrap();
    token.expires_at = Some(token.obtained_at.saturating_sub(720));
    store.set(&key, &token);
}

#[test]
fn client_credentials_then_reuse_then_refresh() {
    let server = TestServer::start();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let (calls_in, seen_in) = (calls.clone(), seen.clone());
    let handle = server.serve(move |request| {
        let n = calls_in.fetch_add(1, Ordering::SeqCst);
        seen_in.lock().unwrap().push(body(&request));
        let refresh = if n == 0 { Some("r1") } else { None };
        StubResponse::new(200)
            .with_header("Content-Type", "application/json")
            .with_body(token_json(&format!("a{n}"), refresh))
    });
    let store = MemoryStore::default();
    let profile = profile(&server, OAuthGrant::ClientCredentials);

    let first = run(&store, &profile, &NoSignIn, Force::None);
    assert_eq!(first.event, Some(TokenEvent::New));
    assert_eq!(first.token().unwrap().access_token, "a0");
    assert!(matches!(
        first.steps[0],
        OAuthStep::Note {
            note: OAuthNote::NoToken,
            ..
        }
    ));
    assert!(matches!(first.steps[1], OAuthStep::Exchange(_)));
    assert!(matches!(
        first.steps[2],
        OAuthStep::Note {
            note: OAuthNote::NewToken { .. },
            ..
        }
    ));

    let second = run(&store, &profile, &NoSignIn, Force::None);
    assert_eq!(second.event, Some(TokenEvent::Reused));
    assert!(second.steps.is_empty());

    expire(&store, &profile);
    let third = run(&store, &profile, &NoSignIn, Force::None);
    assert_eq!(third.event, Some(TokenEvent::Renewed));
    let token = third.token().unwrap();
    assert_eq!(token.access_token, "a1");
    // The server sent no new refresh token: the old one is kept.
    assert_eq!(token.refresh_token.as_deref(), Some("r1"));
    assert!(matches!(
        third.steps[0],
        OAuthStep::Note {
            note: OAuthNote::Expired {
                has_refresh: true,
                ..
            },
            ..
        }
    ));

    let bodies = seen.lock().unwrap().clone();
    assert_eq!(bodies[0], "grant_type=client_credentials&scope=read");
    assert_eq!(
        bodies[1],
        "grant_type=refresh_token&refresh_token=r1&scope=read"
    );
    // The console copy of the refresh is masked.
    let OAuthStep::Exchange(exchange) = &third.steps[1] else {
        panic!("exchange expected");
    };
    let masked = match &exchange.request.body {
        postino_core::ResolvedBody::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        postino_core::ResolvedBody::None => String::new(),
    };
    assert!(masked.contains("refresh_token=••••••••"), "{masked}");
    let response = String::from_utf8_lossy(&exchange.response.as_ref().unwrap().body).into_owned();
    assert!(!response.contains("\"a1\""), "{response}");

    drop(server);
    handle.join().unwrap();
}

#[test]
fn a_rejected_refresh_repeats_the_grant() {
    let server = TestServer::start();
    let handle = server.serve(move |request| {
        let body = body(&request);
        if body.starts_with("grant_type=refresh_token") {
            StubResponse::new(400).with_body(r#"{"error":"invalid_grant"}"#)
        } else {
            StubResponse::new(200).with_body(token_json("fresh", Some("r2")))
        }
    });
    let store = MemoryStore::default();
    let profile = profile(&server, OAuthGrant::Password);
    let key = token_key("ws-123456", "dev", &profile, "");
    store.set(
        &key,
        &TokenSet {
            access_token: "old".into(),
            refresh_token: Some("dead".into()),
            id_token: None,
            token_type: "Bearer".into(),
            expires_at: Some(1),
            obtained_at: 0,
            scope: String::new(),
        },
    );
    let run = run(&store, &profile, &NoSignIn, Force::None);
    assert_eq!(run.event, Some(TokenEvent::Renewed));
    assert_eq!(run.token().unwrap().access_token, "fresh");
    assert!(run.steps.iter().any(|step| matches!(
        step,
        OAuthStep::Note {
            note: OAuthNote::RefreshRejected,
            ..
        }
    )));
    drop(server);
    handle.join().unwrap();
}

#[test]
fn server_errors_are_reported() {
    let server = TestServer::start();
    let handle = server.serve(|_| {
        StubResponse::new(401).with_body(r#"{"error":"invalid_client","error_description":"bad"}"#)
    });
    let store = MemoryStore::default();
    let run = run(
        &store,
        &profile(&server, OAuthGrant::ClientCredentials),
        &NoSignIn,
        Force::None,
    );
    assert!(matches!(
        run.result,
        Err(OAuthError::Server { status: 401, ref error, .. }) if error == "invalid_client"
    ));
    assert!(matches!(
        run.steps.last(),
        Some(OAuthStep::Note {
            note: OAuthNote::Failed { .. },
            ..
        })
    ));
    drop(server);
    handle.join().unwrap();
}

#[test]
fn authorization_code_without_a_ui_needs_a_sign_in() {
    let server = TestServer::start();
    let store = MemoryStore::default();
    let run = run(
        &store,
        &profile(&server, OAuthGrant::AuthorizationCode),
        &NoSignIn,
        Force::None,
    );
    assert!(matches!(run.result, Err(OAuthError::SignInRequired)));
}

/// Plays the browser: follows the authorization URL straight to the redirect, as a provider that
/// signs the user in at once would.
struct FakeBrowser {
    opened: Mutex<Option<SignInPrompt>>,
}

impl SignInHandler for FakeBrowser {
    fn open(&self, prompt: &SignInPrompt) {
        *self.opened.lock().unwrap() = Some(prompt.clone());
        let params = query_params(&prompt.url);
        let get = |name: &str| {
            params
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
                .unwrap_or_default()
        };
        let redirect = format!(
            "{}?code=the-code&state={}",
            get("redirect_uri"),
            get("state")
        );
        std::thread::spawn(move || {
            let rest = redirect.trim_start_matches("http://");
            let (address, path) = rest.split_at(rest.find('/').unwrap());
            use std::io::{Read, Write};
            let mut stream = std::net::TcpStream::connect(address).unwrap();
            write!(
                stream,
                "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            let mut answer = String::new();
            let _ = stream.read_to_string(&mut answer);
        });
    }

    fn poll(&self) -> SignInPoll {
        SignInPoll::Waiting
    }
}

#[test]
fn authorization_code_with_pkce_signs_in_through_the_loopback() {
    let server = TestServer::start();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen_in = seen.clone();
    let handle = server.serve(move |request| {
        seen_in.lock().unwrap().push(body(&request));
        StubResponse::new(200).with_body(token_json("signed", Some("r")))
    });
    let store = MemoryStore::default();
    let mut profile = profile(&server, OAuthGrant::AuthorizationCode);
    profile.client_secret.clear();
    let browser = FakeBrowser {
        opened: Mutex::new(None),
    };
    let run = run(&store, &profile, &browser, Force::None);
    assert_eq!(run.event, Some(TokenEvent::New));
    assert_eq!(run.token().unwrap().access_token, "signed");

    let prompt = browser.opened.lock().unwrap().clone().unwrap();
    assert_eq!(prompt.profile, "acme");
    assert!(prompt.listening.unwrap().starts_with("127.0.0.1:"));
    let params = query_params(&prompt.url);
    let get = |name: &str| {
        params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(get("code_challenge_method").as_deref(), Some("S256"));
    assert_eq!(get("client_id").as_deref(), Some("postino"));

    let exchange = seen.lock().unwrap()[0].clone();
    let form = query_params(&format!("x?{exchange}"));
    let field = |name: &str| {
        form.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(field("grant_type").as_deref(), Some("authorization_code"));
    assert_eq!(field("code").as_deref(), Some("the-code"));
    assert_eq!(field("client_id").as_deref(), Some("postino"));
    let verifier = field("code_verifier").unwrap();
    assert_eq!(
        postino_oauth::Pkce::from_verifier(verifier.clone()).challenge,
        get("code_challenge").unwrap()
    );
    drop(server);
    handle.join().unwrap();
}
