//! The `send` function and the [`Client`] type: hand a [`ResolvedRequest`] to `ureq` and turn
//! the result back into plain `postino-core` types.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Instant;

use postino_core::{ResolvedBody, ResolvedField, ResolvedRequest, Response, log_safe};

use crate::error::HttpError;
use crate::options::{InvalidCertificates, SendOptions};

/// Sends a fully resolved request over HTTP and returns the response, on a connection of its
/// own: it is opened for this request (with its TLS handshake over HTTPS) and closed afterwards.
/// [`Client::send`] keeps connections open between requests instead.
///
/// 4xx and 5xx status codes are returned as a normal [`Response`], not as an [`HttpError`]:
/// only failures that prevent us from getting any response at all (an invalid request, a
/// timeout, too many redirects, a connection failure, ...) are errors. The returned response's
/// `time` is the total time spent sending the request and reading the whole response body.
///
/// With [`InvalidCertificates::SendWithWarning`] and a server certificate that is rejected
/// (self-signed, expired, wrong host, ...), the request is sent once more with verification
/// disabled, and the returned response carries a [`Response::tls_warning`]. This is safe for
/// any method: a certificate error aborts the TLS handshake, before any request byte is written.
///
/// When the TLS handshake fails because the server only offers what rustls does not implement
/// (RSA key exchange, CBC cipher suites, TLS 1.0 or 1.1), the request is sent once more with
/// the system TLS library (OpenSSL on Linux, SChannel on Windows, Security.framework on macOS),
/// verifying the certificate the same way, and the returned response has
/// [`Response::legacy_tls`] set. The same certificate fallback applies to that connection.
pub fn send(request: &ResolvedRequest, options: &SendOptions) -> Result<Response, HttpError> {
    // A client of its own, dropped with its connection once the response is read.
    Client::new(options.clone()).send(request)
}

/// Sends requests with keep-alive: the connection a request opens stays open after its response
/// and is reused by the next request to the same scheme, host and port, skipping the TCP connect
/// and the TLS handshake.
///
/// A client holds one connection pool. `ureq` keeps at most 3 idle connections per host and 10
/// overall in it, so a client is meant to be used by one sender at a time (a load test creates
/// one per virtual user), not shared by many threads sending at once: most of their connections
/// would be closed when they go back to the pool.
///
/// With [`InvalidCertificates::SendWithWarning`], a host whose certificate is rejected once is
/// remembered, and its later requests go straight to the connection without verification, with
/// the same [`Response::tls_warning`]. A host that only works with the system TLS library is
/// remembered the same way.
pub struct Client {
    options: SendOptions,
    /// Sends every request, verifying certificates unless they are always accepted.
    agent: ureq::Agent,
    /// Sends without verification, built the first time a certificate is rejected.
    unverified: OnceLock<ureq::Agent>,
    /// Sends with the system TLS library, built the first time a server needs it.
    legacy: OnceLock<ureq::Agent>,
    /// Sends with the system TLS library without verification.
    legacy_unverified: OnceLock<ureq::Agent>,
    /// How each origin (`scheme://host:port`) that needed a fallback is sent to.
    fallback_origins: Mutex<HashMap<String, TlsFallback>>,
}

/// The fallbacks a request needed to get through the TLS handshake.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TlsFallback {
    /// Sent with the system TLS library, see [`is_legacy_tls_error`].
    legacy: bool,
    /// Sent without verification, because the certificate was rejected for this reason.
    warning: Option<String>,
}

impl Client {
    /// A client that sends with `options`. No connection is opened until the first request.
    #[must_use]
    pub fn new(options: SendOptions) -> Self {
        let verify = options.invalid_certificates != InvalidCertificates::Accept;
        Self {
            agent: build_agent(&options, verify, false),
            options,
            unverified: OnceLock::new(),
            legacy: OnceLock::new(),
            legacy_unverified: OnceLock::new(),
            fallback_origins: Mutex::new(HashMap::new()),
        }
    }

    /// Sends a fully resolved request and returns the response, reusing an open connection to
    /// its origin when there is one. Errors and certificate handling are those of [`send`].
    pub fn send(&self, request: &ResolvedRequest) -> Result<Response, HttpError> {
        let (response, started, fallback) = self.start(request)?;
        finish(response, started, fallback, self.options.max_response_size)
    }

    /// [`Self::send`], writing the response body to `sink` as it arrives instead of keeping it
    /// in memory, for downloads. `progress` is called after every chunk with the bytes written
    /// so far and the `Content-Length`, when the server sent one. The returned response has an
    /// empty `body`; its `size` is the number of bytes written. The body limit of the options
    /// still applies.
    pub fn download(
        &self,
        request: &ResolvedRequest,
        sink: &mut dyn Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<Response, HttpError> {
        let (mut response, started, fallback) = self.start(request)?;
        let status = response.status().as_u16();
        let headers = collect_headers(response.headers());
        let total = response.body().content_length();
        let mut reader = response
            .body_mut()
            .with_config()
            .limit(self.options.max_response_size)
            .reader();
        let mut buffer = vec![0u8; 64 * 1024];
        let mut written: u64 = 0;
        progress(0, total);
        loop {
            let read = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(map_read_error(error, self.options.max_response_size)),
            };
            sink.write_all(&buffer[..read]).map_err(HttpError::Write)?;
            written += read as u64;
            progress(written, total);
        }
        sink.flush().map_err(HttpError::Write)?;
        Ok(Response {
            status,
            headers,
            body: Vec::new(),
            time: started.elapsed(),
            size: usize::try_from(written).unwrap_or(usize::MAX),
            tls_warning: fallback.warning,
            legacy_tls: fallback.legacy,
        })
    }

    /// Sends `request` and waits for the response head, falling back to an unverified
    /// connection or to the system TLS library as [`send`] describes. Returns the response, when
    /// it started and the fallbacks it needed.
    fn start(
        &self,
        request: &ResolvedRequest,
    ) -> Result<(ureq::http::Response<ureq::Body>, Instant, TlsFallback), HttpError> {
        if log::log_enabled!(log::Level::Trace) {
            log_request(request);
        }
        let http_request = build_http_request(request)?;
        let started = Instant::now();

        let origin = origin_of(&http_request);
        let known = self.origins().get(&origin).cloned();
        let (response, fallback) = match known {
            Some(fallback) => {
                let response = run(self.agent_for(&fallback), &http_request);
                (response.map_err(map_ureq_error)?, fallback)
            }
            None => self.first_send(&http_request, origin)?,
        };
        if let Some(trace) = &self.options.trace {
            trace_redirects(trace, &response);
        }
        Ok((response, started, fallback))
    }

    /// Sends a request to an origin with no known fallback, trying the fallbacks [`send`]
    /// describes when the handshake fails, and remembers the one that worked. Every retry is
    /// safe for any method: each one follows a failed handshake, before any request byte.
    fn first_send(
        &self,
        request: &ureq::http::Request<Vec<u8>>,
        origin: String,
    ) -> Result<(ureq::http::Response<ureq::Body>, TlsFallback), HttpError> {
        let accept_with_warning =
            self.options.invalid_certificates == InvalidCertificates::SendWithWarning;
        let error = match run(&self.agent, request) {
            Ok(response) => return Ok((response, TlsFallback::default())),
            Err(error) => error,
        };
        if accept_with_warning && is_certificate_error(&error) {
            let warning = certificate_reason(&error);
            log::debug!(
                "TLS certificate not verified: {warning}, sending again without verification"
            );
            let fallback = TlsFallback {
                legacy: false,
                warning: Some(warning),
            };
            let response = run(self.agent_for(&fallback), request).map_err(map_ureq_error)?;
            self.origins().insert(origin, fallback.clone());
            return Ok((response, fallback));
        }
        if !is_legacy_tls_error(&error) {
            return Err(map_ureq_error(error));
        }

        log::info!(
            "TLS handshake with {origin} failed ({error}), sending again with the system TLS library"
        );
        if let Some(trace) = &self.options.trace {
            trace.emit(&crate::TraceEvent::LegacyTls);
        }
        let fallback = TlsFallback {
            legacy: true,
            warning: None,
        };
        let legacy_error = match run(self.agent_for(&fallback), request) {
            Ok(response) => {
                self.origins().insert(origin, fallback.clone());
                return Ok((response, fallback));
            }
            Err(error) => error,
        };
        // The system library does not tell a rejected certificate from other handshake
        // failures in a portable way: try once more without verification, and keep its reason.
        if accept_with_warning && matches!(legacy_error, ureq::Error::NativeTls(_)) {
            let fallback = TlsFallback {
                legacy: true,
                warning: Some(native_certificate_reason(&legacy_error)),
            };
            if let Ok(response) = run(self.agent_for(&fallback), request) {
                log::debug!("TLS certificate not verified by the system TLS library");
                self.origins().insert(origin, fallback.clone());
                return Ok((response, fallback));
            }
        }
        Err(map_ureq_error(legacy_error))
    }

    /// The agent that sends with `fallback`, built the first time it is needed.
    fn agent_for(&self, fallback: &TlsFallback) -> &ureq::Agent {
        match (fallback.legacy, fallback.warning.is_some()) {
            (false, false) => &self.agent,
            (false, true) => self
                .unverified
                .get_or_init(|| build_agent(&self.options, false, false)),
            (true, unverified) => {
                let verify =
                    !unverified && self.options.invalid_certificates != InvalidCertificates::Accept;
                let cell = if unverified {
                    &self.legacy_unverified
                } else {
                    &self.legacy
                };
                cell.get_or_init(|| build_agent(&self.options, verify, true))
            }
        }
    }

    /// The origins that needed a fallback. Recovers from a poisoned lock: the map is only read
    /// and inserted into, so it is never left half updated.
    fn origins(&self) -> std::sync::MutexGuard<'_, HashMap<String, TlsFallback>> {
        self.fallback_origins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Reports every redirect `response` followed: its history is the first URL, then each one it
/// was redirected to.
fn trace_redirects(trace: &crate::Trace, response: &ureq::http::Response<ureq::Body>) {
    use ureq::ResponseExt as _;
    for uri in response
        .get_redirect_history()
        .unwrap_or_default()
        .iter()
        .skip(1)
    {
        let url = log_safe::url_for_log(&uri.to_string()).into_owned();
        trace.emit(&crate::TraceEvent::Redirected(url));
    }
}

/// The `scheme://host:port` a request goes to, the unit connections are reused for.
fn origin_of(request: &ureq::http::Request<Vec<u8>>) -> String {
    let uri = request.uri();
    format!(
        "{}://{}",
        uri.scheme_str().unwrap_or_default(),
        uri.authority().map_or("", |authority| authority.as_str())
    )
}

/// Logs the request line and headers at Trace, with sensitive header values redacted and the
/// query string left out.
fn log_request(request: &ResolvedRequest) {
    log::trace!(
        "sending {} {}",
        request.method,
        log_safe::url_for_log(&request.url)
    );
    for header in &request.headers {
        log::trace!(
            "  {}: {}",
            header.name,
            log_safe::header_value_for_log(&header.name, &header.value)
        );
    }
}

/// Runs one request on `agent`.
fn run(
    agent: &ureq::Agent,
    request: &ureq::http::Request<Vec<u8>>,
) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    agent.run(request.clone())
}

/// Reads the whole response body, up to `max_response_size` bytes, and turns it into a
/// [`Response`].
fn finish(
    mut response: ureq::http::Response<ureq::Body>,
    started: Instant,
    fallback: TlsFallback,
    max_response_size: u64,
) -> Result<Response, HttpError> {
    let status = response.status().as_u16();
    let headers = collect_headers(response.headers());
    let mut body = response
        .body_mut()
        .with_config()
        .limit(max_response_size)
        .read_to_vec()
        .map_err(|error| match error {
            ureq::Error::BodyExceedsLimit(limit) => HttpError::BodyTooLarge(limit),
            other => map_ureq_error(other),
        })?;
    // The Vec grew by doubling: give the spare capacity back, a big body is kept for as long as
    // its response is shown.
    body.shrink_to_fit();
    let time = started.elapsed();
    let size = body.len();

    Ok(Response {
        status,
        headers,
        body,
        time,
        size,
        tls_warning: fallback.warning,
        legacy_tls: fallback.legacy,
    })
}

/// Maps an error of the body reader of [`Client::download`]: `ureq` wraps its own errors, the
/// body limit among them, in the `io::Error` it returns.
fn map_read_error(error: std::io::Error, max_response_size: u64) -> HttpError {
    match error.downcast::<ureq::Error>() {
        Ok(ureq::Error::BodyExceedsLimit(_)) => HttpError::BodyTooLarge(max_response_size),
        Ok(other) => map_ureq_error(other),
        Err(error) => HttpError::Network(ureq::Error::Io(error)),
    }
}

/// Checks a proxy URL for [`SendOptions::proxy`], with the reason it is not valid.
pub fn check_proxy(proxy: &str) -> Result<(), String> {
    ureq::Proxy::new(proxy)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// The `rustls` error inside a `ureq` error, if any. `ureq` reports handshake failures either
/// as [`ureq::Error::Rustls`] or as an I/O error wrapping the `rustls` one.
fn rustls_error(error: &ureq::Error) -> Option<&rustls::Error> {
    match error {
        ureq::Error::Rustls(inner) => Some(inner),
        ureq::Error::Io(io) => io.get_ref()?.downcast_ref::<rustls::Error>(),
        _ => None,
    }
}

/// Whether `error` means the server's certificate was rejected (as opposed to any other TLS or
/// network failure).
fn is_certificate_error(error: &ureq::Error) -> bool {
    matches!(
        rustls_error(error),
        Some(rustls::Error::InvalidCertificate(_))
    )
}

/// Whether the TLS handshake failed because the server only offers what rustls does not
/// implement: RSA key exchange, CBC cipher suites, TLS 1.0 or 1.1. Such a server answers the
/// client hello with a `handshake_failure` or `protocol_version` alert, or picks a version rustls
/// refuses. The system TLS library still speaks these, like curl does (#305).
fn is_legacy_tls_error(error: &ureq::Error) -> bool {
    use rustls::AlertDescription;
    matches!(
        rustls_error(error),
        Some(
            rustls::Error::AlertReceived(
                AlertDescription::HandshakeFailure
                    | AlertDescription::ProtocolVersion
                    | AlertDescription::InsufficientSecurity
            ) | rustls::Error::PeerIncompatible(_)
        )
    )
}

/// The reason the system TLS library rejected a certificate. OpenSSL appends it in parentheses,
/// such as `... certificate verify failed ... (self-signed certificate)`. Other libraries give
/// their whole message.
fn native_certificate_reason(error: &ureq::Error) -> String {
    let text = match error {
        ureq::Error::NativeTls(inner) => inner.to_string(),
        other => other.to_string(),
    };
    match text
        .strip_suffix(')')
        .and_then(|rest| rest.rsplit_once(" ("))
    {
        Some((_, reason)) => reason.to_string(),
        None => text,
    }
}

/// A short description of why the certificate was rejected.
fn certificate_reason(error: &ureq::Error) -> String {
    match rustls_error(error) {
        Some(rustls::Error::InvalidCertificate(reason)) => match reason {
            rustls::CertificateError::UnknownIssuer => "unknown issuer (self-signed?)".to_string(),
            rustls::CertificateError::Expired | rustls::CertificateError::ExpiredContext { .. } => {
                "certificate expired".to_string()
            }
            rustls::CertificateError::NotValidYet
            | rustls::CertificateError::NotValidYetContext { .. } => {
                "certificate not valid yet".to_string()
            }
            rustls::CertificateError::NotValidForName
            | rustls::CertificateError::NotValidForNameContext { .. } => {
                "certificate does not match the host name".to_string()
            }
            other => format!("{other:?}"),
        },
        _ => error.to_string(),
    }
}

/// Builds a `ureq` agent configured from `options`.
///
/// This disables `ureq`'s default behavior of treating 4xx/5xx as errors, allows the
/// non-standard HTTP methods a `.postino` file may use, and applies the timeout, redirect and
/// TLS verification settings. With `legacy`, TLS goes through the system library instead of
/// rustls, see [`is_legacy_tls_error`].
fn build_agent(options: &SendOptions, verify_tls: bool, legacy: bool) -> ureq::Agent {
    let root_certs = root_certs_of(options);
    let provider = if legacy {
        ureq::tls::TlsProvider::NativeTls
    } else {
        ureq::tls::TlsProvider::Rustls
    };
    let tls_config = ureq::tls::TlsConfig::builder()
        .provider(provider)
        .root_certs(root_certs)
        .disable_verification(!verify_tls)
        .build();

    let max_redirects = if options.follow_redirects {
        options.max_redirects
    } else {
        0
    };

    let mut config = ureq::Agent::config_builder();
    if let Some(proxy) = options.proxy.as_deref() {
        match ureq::Proxy::new(proxy) {
            Ok(proxy) => config = config.proxy(Some(proxy)),
            Err(error) => log::warn!("ignoring the proxy setting: {error}"),
        }
    }
    let config = config
        .http_status_as_error(false)
        .allow_non_standard_methods(true)
        .max_redirects(max_redirects)
        .timeout_global(Some(options.timeout))
        .tls_config(tls_config)
        .save_redirect_history(options.trace.is_some())
        .build();

    // The traced connector does TLS with rustls: a legacy agent is not traced past the
    // `LegacyTls` event its first request emits.
    match &options.trace {
        Some(trace) if !legacy => ureq::Agent::with_parts(
            config,
            crate::trace::connector(trace.clone(), root_certs_of(options), verify_tls),
            ureq::unversioned::resolver::DefaultResolver::default(),
        ),
        _ => config.new_agent(),
    }
}

/// The roots certificates are verified against, see [`SendOptions::system_certificates`].
fn root_certs_of(options: &SendOptions) -> ureq::tls::RootCerts {
    if options.system_certificates {
        system_roots()
    } else {
        ureq::tls::RootCerts::WebPki
    }
}

/// The roots of [`SendOptions::system_certificates`] on Windows and macOS: the platform verifier,
/// which asks the OS to verify each certificate and costs nothing to create.
#[cfg(not(all(unix, not(target_vendor = "apple"))))]
fn system_roots() -> ureq::tls::RootCerts {
    ureq::tls::RootCerts::PlatformVerifier
}

/// The roots of [`SendOptions::system_certificates`] on Linux and the BSDs: the system bundle
/// (`SSL_CERT_FILE` and `SSL_CERT_DIR` when set), which is all the platform verifier uses there.
/// The platform verifier reads and parses the bundle again for every agent, about 3.5 ms, and
/// [`send`] creates one per request, so a load test without keep-alive would pay it on every
/// request (measured on 2026-10-18, #114). The bundle is read once per process instead. Without
/// any readable system certificate, the bundled Mozilla roots are used.
#[cfg(all(unix, not(target_vendor = "apple")))]
fn system_roots() -> ureq::tls::RootCerts {
    static ROOTS: OnceLock<ureq::tls::RootCerts> = OnceLock::new();
    ROOTS
        .get_or_init(|| {
            let result = rustls_native_certs::load_native_certs();
            for error in &result.errors {
                log::warn!("could not load a system CA certificate: {error}");
            }
            if result.certs.is_empty() {
                log::warn!("no system CA certificates found, using the bundled roots");
                return ureq::tls::RootCerts::WebPki;
            }
            log::debug!("loaded {} system CA certificates", result.certs.len());
            let certs = result
                .certs
                .iter()
                .map(|cert| ureq::tls::Certificate::from_der(cert).to_owned())
                .collect();
            ureq::tls::RootCerts::Specific(std::sync::Arc::new(certs))
        })
        .clone()
}

/// Turns a [`ResolvedRequest`] into an `http` crate request ready to hand to the `ureq` agent.
fn build_http_request(
    request: &ResolvedRequest,
) -> Result<ureq::http::Request<Vec<u8>>, HttpError> {
    let method = request.method.to_string();
    let mut builder = ureq::http::Request::builder()
        .method(method.as_str())
        .uri(&request.url);

    for header in &request.headers {
        builder = builder.header(&header.name, &header.value);
    }

    let body = match &request.body {
        ResolvedBody::None => Vec::new(),
        ResolvedBody::Bytes(bytes) => bytes.clone(),
    };

    builder
        .body(body)
        .map_err(|error| HttpError::InvalidRequest(error.to_string()))
}

/// Reads the response headers into the plain [`ResolvedField`] list `postino-core` expects.
///
/// Note: the `http` crate preserves insertion order for repeated values of the same header
/// name, but its documented iteration order across different header names is not guaranteed to
/// match the exact order the bytes arrived on the wire.
fn collect_headers(headers: &ureq::http::HeaderMap) -> Vec<ResolvedField> {
    headers
        .iter()
        .map(|(name, value)| ResolvedField {
            name: name.as_str().to_string(),
            value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
        })
        .collect()
}

/// Maps a `ureq` error to our own [`HttpError`], giving the cases `postino-http` cares about a
/// dedicated variant and keeping everything else under [`HttpError::Network`].
fn map_ureq_error(error: ureq::Error) -> HttpError {
    match error {
        ureq::Error::Timeout(_) => HttpError::Timeout,
        ureq::Error::TooManyRedirects => HttpError::TooManyRedirects,
        other if is_certificate_error(&other) => HttpError::Certificate(certificate_reason(&other)),
        other => HttpError::Network(other),
    }
}
