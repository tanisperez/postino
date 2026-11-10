//! The authorization code grant in the system browser (RFC 8252): PKCE, the authorization URL,
//! the local listener that receives the redirect, and reading a pasted redirect URL.

use std::time::{Duration, Instant};

use base64::Engine;
use postino_core::OAuthRedirect;
use postino_core::functions::url_encode;
use sha2::{Digest, Sha256};

use crate::error::OAuthError;
use crate::resolve::ResolvedProfile;

/// How long Postino waits for the user to sign in before giving up and closing the port.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// How often the wait checks for a cancel or a pasted URL.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// `count` random bytes from the operating system, encoded as unpadded base64url.
fn random_token(count: usize) -> String {
    let mut bytes = vec![0u8; count];
    // The OS generator does not fail on any supported platform; if it ever did, a verifier made
    // of zeros would still be accepted by the server, only less secret, so it is logged.
    if let Err(error) = getrandom::fill(&mut bytes) {
        log::warn!("oauth: no random bytes from the system: {error}");
    }
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// A PKCE pair (RFC 7636): the verifier kept by Postino and its S256 challenge sent in the URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    /// 43 characters, sent with the code exchange.
    pub verifier: String,
    /// `base64url(sha256(verifier))`, sent in the authorization URL.
    pub challenge: String,
}

impl Pkce {
    /// A new pair with a fresh random verifier, one per sign-in.
    pub fn new() -> Self {
        Self::from_verifier(random_token(32))
    }

    /// The pair of an existing `verifier`.
    pub fn from_verifier(verifier: String) -> Self {
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        Pkce {
            verifier,
            challenge,
        }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

/// A new random `state` for one sign-in.
pub fn new_state() -> String {
    random_token(16)
}

/// The URL opened in the browser.
pub fn authorization_url(
    profile: &ResolvedProfile,
    redirect_uri: &str,
    state: &str,
    pkce: Option<&Pkce>,
) -> String {
    let mut params: Vec<(&str, &str)> = vec![
        ("response_type", "code"),
        ("client_id", &profile.client_id),
        ("redirect_uri", redirect_uri),
    ];
    if !profile.scope.is_empty() {
        params.push(("scope", &profile.scope));
    }
    params.push(("state", state));
    if let Some(pkce) = pkce {
        params.push(("code_challenge", &pkce.challenge));
        params.push(("code_challenge_method", "S256"));
    }
    let mut query: Vec<String> = params
        .iter()
        .map(|(name, value)| format!("{}={}", url_encode(name), url_encode(value)))
        .collect();
    query.extend(
        profile
            .auth_params
            .iter()
            .map(|(name, value)| format!("{}={}", url_encode(name), url_encode(value))),
    );
    let separator = if profile.auth_url.contains('?') {
        '&'
    } else {
        '?'
    };
    format!("{}{separator}{}", profile.auth_url, query.join("&"))
}

/// Percent-decodes a query component, `+` meaning a space.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => out.push(b' '),
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The query parameters of `url` (the part after `?`, without any `#fragment`), decoded.
pub fn query_params(url: &str) -> Vec<(String, String)> {
    let without_fragment = url.split('#').next().unwrap_or_default();
    let Some((_, query)) = without_fragment.split_once('?') else {
        return Vec::new();
    };
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(name), percent_decode(value))
        })
        .collect()
}

/// Reads the authorization code from the URL the browser was redirected to, checking `state`.
pub fn code_from_redirect(url: &str, state: &str) -> Result<String, OAuthError> {
    let params = query_params(url.trim());
    let get = |name: &str| {
        params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    if let Some(error) = get("error") {
        return Err(OAuthError::Denied {
            error,
            description: get("error_description"),
        });
    }
    let Some(code) = get("code").filter(|code| !code.is_empty()) else {
        return Err(OAuthError::NoCode);
    };
    if get("state").as_deref() != Some(state) {
        return Err(OAuthError::StateMismatch);
    }
    Ok(code)
}

/// A local `http://host:port/path` URL, split.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalUrl {
    port: u16,
    path: String,
}

/// Reads a fixed redirect URL: `http://localhost` or `http://127.0.0.1`, with a port.
fn parse_local_url(url: &str) -> Result<LocalUrl, OAuthError> {
    let rest = url
        .trim()
        .strip_prefix("http://")
        .ok_or(OAuthError::InvalidRedirectUrl)?;
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or(OAuthError::InvalidRedirectUrl)?;
    if host != "localhost" && host != "127.0.0.1" {
        return Err(OAuthError::InvalidRedirectUrl);
    }
    let port = port
        .parse::<u16>()
        .map_err(|_| OAuthError::InvalidRedirectUrl)?;
    let path = path.split(['?', '#']).next().unwrap_or("/").to_string();
    Ok(LocalUrl { port, path })
}

/// The texts of the page the listener answers the browser with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackPage {
    /// The heading, for example "Signed in to acme-sso".
    pub title: String,
    /// The line under it.
    pub body: String,
}

/// Escapes `text` for HTML.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The page shown in the browser after the redirect (design 3c, step 2).
pub fn callback_html(page: &CallbackPage) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Postino</title><style>body{{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;background:#fcfcfd;color:#1b1c22;font-family:system-ui,-apple-system,'Segoe UI',sans-serif;}}main{{display:flex;flex-direction:column;align-items:center;gap:10px;padding:24px;text-align:center;}}.logo{{width:40px;height:40px;border-radius:10px;background:#4f57d8;color:#fff;font-weight:700;font-size:20px;display:flex;align-items:center;justify-content:center;}}h1{{margin:0;font-size:16px;font-weight:600;}}p{{margin:0;color:#5c606e;font-size:14px;}}</style></head><body><main><div class=\"logo\">P</div><h1>{}</h1><p>{}</p></main></body></html>",
        escape_html(&page.title),
        escape_html(&page.body)
    )
}

/// Where the redirect arrives.
pub struct Listener {
    server: Option<tiny_http::Server>,
    path: String,
    redirect_uri: String,
    address: Option<String>,
}

impl Listener {
    /// Starts listening for `redirect`: a free port on 127.0.0.1 for the loopback redirect, the
    /// port of `redirect_url` for a fixed one, nothing for a manual one (the URL is pasted).
    pub fn start(redirect: OAuthRedirect, redirect_url: &str) -> Result<Self, OAuthError> {
        match redirect {
            OAuthRedirect::Loopback => {
                let server =
                    tiny_http::Server::http("127.0.0.1:0").map_err(|error| OAuthError::Listen {
                        address: "127.0.0.1".to_string(),
                        source: std::io::Error::other(error.to_string()),
                    })?;
                let port = server
                    .server_addr()
                    .to_ip()
                    .map(|address| address.port())
                    .unwrap_or_default();
                Ok(Listener {
                    server: Some(server),
                    path: "/callback".to_string(),
                    redirect_uri: format!("http://127.0.0.1:{port}/callback"),
                    address: Some(format!("127.0.0.1:{port}")),
                })
            }
            OAuthRedirect::Fixed => {
                let url = parse_local_url(redirect_url)?;
                let address = format!("127.0.0.1:{}", url.port);
                let server =
                    tiny_http::Server::http(&address).map_err(|error| OAuthError::Listen {
                        address: address.clone(),
                        source: std::io::Error::other(error.to_string()),
                    })?;
                Ok(Listener {
                    server: Some(server),
                    path: url.path,
                    redirect_uri: redirect_url.trim().to_string(),
                    address: Some(address),
                })
            }
            OAuthRedirect::Manual => Ok(Listener {
                server: None,
                path: String::new(),
                redirect_uri: redirect_url.trim().to_string(),
                address: None,
            }),
        }
    }

    /// The `redirect_uri` sent to the server.
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// The address it listens on, `None` for a manual redirect.
    pub fn address(&self) -> Option<&str> {
        self.address.as_deref()
    }

    /// Waits up to [`POLL_INTERVAL`] for the redirect. Returns the full URL it arrived at, after
    /// answering the browser with `page` (built from the URL, so it can tell success from
    /// failure). Requests to other paths, such as a favicon, get a 404.
    pub fn poll(&self, page: &dyn Fn(&str) -> CallbackPage) -> Option<String> {
        let server = self.server.as_ref()?;
        let request = server.recv_timeout(POLL_INTERVAL).ok().flatten()?;
        let target = request.url().to_string();
        let path = target.split('?').next().unwrap_or_default();
        if path != self.path {
            let _ = request.respond(tiny_http::Response::empty(404));
            return None;
        }
        let url = format!("http://localhost{target}");
        let html = callback_html(&page(&url));
        let mut response = tiny_http::Response::from_string(html);
        if let Ok(header) =
            tiny_http::Header::from_bytes("Content-Type", "text/html; charset=utf-8")
        {
            response.add_header(header);
        }
        let _ = request.respond(response);
        Some(url)
    }
}

/// What the person waiting on the sign-in did, polled by [`wait_for_redirect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInPoll {
    /// Nothing yet.
    Waiting,
    /// They pasted the URL the browser ended at.
    Pasted(String),
    /// They cancelled.
    Cancelled,
}

/// Waits for the redirect on `listener`, or a pasted URL, or a cancel from `poll`, up to
/// [`SIGN_IN_TIMEOUT`]. Returns the redirect URL.
pub fn wait_for_redirect(
    listener: &Listener,
    poll: &dyn Fn() -> SignInPoll,
    page: &dyn Fn(&str) -> CallbackPage,
    timeout: Duration,
) -> Result<String, OAuthError> {
    let started = Instant::now();
    loop {
        match poll() {
            SignInPoll::Cancelled => return Err(OAuthError::Cancelled),
            SignInPoll::Pasted(url) => return Ok(url),
            SignInPoll::Waiting => {}
        }
        if let Some(url) = listener.poll(page) {
            return Ok(url);
        }
        if listener.server.is_none() {
            std::thread::sleep(POLL_INTERVAL);
        }
        if started.elapsed() >= timeout {
            return Err(OAuthError::TimedOut);
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::exchange::tests::profile;
    use postino_core::ClientAuth;
    use pretty_assertions::assert_eq;

    #[test]
    fn pkce_matches_the_rfc_example() {
        // RFC 7636, appendix B.
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into());
        assert_eq!(
            pkce.challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let fresh = Pkce::new();
        assert_eq!(fresh.verifier.len(), 43);
        assert_ne!(fresh.verifier, Pkce::new().verifier);
        assert_ne!(new_state(), new_state());
    }

    #[test]
    fn the_authorization_url_carries_every_parameter() {
        let mut profile = profile(ClientAuth::BasicHeader, "");
        profile.auth_url = "https://auth/authorize?tenant=a".into();
        profile.auth_params = vec![("prompt".into(), "login".into())];
        let pkce = Pkce::from_verifier("v".into());
        let url = authorization_url(&profile, "http://127.0.0.1:5/callback", "st", Some(&pkce));
        assert_eq!(
            url,
            format!(
                "https://auth/authorize?tenant=a&response_type=code&client_id=my%20id&redirect_uri=http%3A%2F%2F127.0.0.1%3A5%2Fcallback&scope=read&state=st&code_challenge={}&code_challenge_method=S256&prompt=login",
                pkce.challenge
            )
        );
    }

    #[test]
    fn redirects_are_read() {
        assert_eq!(
            code_from_redirect("http://127.0.0.1:5/callback?code=a%2Bb+c&state=st", "st")
                .expect("code"),
            "a+b c"
        );
        assert!(matches!(
            code_from_redirect("https://app/cb?code=x&state=other", "st"),
            Err(OAuthError::StateMismatch)
        ));
        assert!(matches!(
            code_from_redirect("https://app/cb?state=st", "st"),
            Err(OAuthError::NoCode)
        ));
        assert!(matches!(
            code_from_redirect("https://app/cb?error=redirect_uri_mismatch&state=st", "st"),
            Err(OAuthError::Denied { ref error, .. }) if error == "redirect_uri_mismatch"
        ));
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn fixed_redirect_urls() {
        assert_eq!(
            parse_local_url("http://localhost:8765/callback").expect("url"),
            LocalUrl {
                port: 8765,
                path: "/callback".into()
            }
        );
        assert!(parse_local_url("https://localhost:8765/callback").is_err());
        assert!(parse_local_url("http://example.com:80/").is_err());
        assert!(parse_local_url("http://localhost/callback").is_err());
    }

    #[test]
    fn the_listener_answers_the_redirect() {
        let listener = Listener::start(OAuthRedirect::Loopback, "").expect("listen");
        let address = listener.address().expect("address").to_string();
        assert!(listener.redirect_uri().starts_with("http://127.0.0.1:"));
        let client = std::thread::spawn(move || {
            let mut stream = std::net::TcpStream::connect(&address).expect("connect");
            use std::io::{Read, Write};
            write!(
                stream,
                "GET /callback?code=abc&state=st HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
            )
            .expect("write");
            let mut answer = String::new();
            stream.read_to_string(&mut answer).expect("read");
            answer
        });
        let page = |_: &str| CallbackPage {
            title: "Signed in to <acme>".into(),
            body: "Close".into(),
        };
        let url = wait_for_redirect(&listener, &|| SignInPoll::Waiting, &page, SIGN_IN_TIMEOUT)
            .expect("redirect");
        assert_eq!(code_from_redirect(&url, "st").expect("code"), "abc");
        let answer = client.join().expect("client");
        assert!(answer.contains("Signed in to &lt;acme&gt;"), "{answer}");
    }

    #[test]
    fn cancel_and_paste_end_the_wait() {
        let listener = Listener::start(OAuthRedirect::Manual, "https://app/cb").expect("manual");
        let page = |_: &str| CallbackPage {
            title: String::new(),
            body: String::new(),
        };
        assert!(matches!(
            wait_for_redirect(&listener, &|| SignInPoll::Cancelled, &page, SIGN_IN_TIMEOUT),
            Err(OAuthError::Cancelled)
        ));
        assert_eq!(
            wait_for_redirect(
                &listener,
                &|| SignInPoll::Pasted("https://app/cb?code=1".into()),
                &page,
                SIGN_IN_TIMEOUT
            )
            .expect("pasted"),
            "https://app/cb?code=1"
        );
        assert!(matches!(
            wait_for_redirect(&listener, &|| SignInPoll::Waiting, &page, Duration::ZERO),
            Err(OAuthError::TimedOut)
        ));
    }
}
