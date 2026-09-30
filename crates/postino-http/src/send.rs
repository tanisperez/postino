//! The `send` function: hands a [`ResolvedRequest`] to `ureq` and turns the result back into
//! plain `postino-core` types.

use std::time::Instant;

use postino_core::{ResolvedBody, ResolvedField, ResolvedRequest, Response};

use crate::error::HttpError;
use crate::options::{InvalidCertificates, SendOptions};

/// Sends a fully resolved request over HTTP and returns the response.
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
pub fn send(request: &ResolvedRequest, options: &SendOptions) -> Result<Response, HttpError> {
    let http_request = build_http_request(request)?;
    let started = Instant::now();

    let verify = options.invalid_certificates != InvalidCertificates::Accept;
    let fallback = options.invalid_certificates == InvalidCertificates::SendWithWarning;
    let (response, tls_warning) = match run(&build_agent(options, verify), &http_request) {
        Err(error) if fallback && is_certificate_error(&error) => {
            let warning = format!(
                "TLS certificate not verified: {}",
                certificate_reason(&error)
            );
            let retry = run(&build_agent(options, false), &http_request).map_err(map_ureq_error)?;
            (retry, Some(warning))
        }
        other => (other.map_err(map_ureq_error)?, None),
    };
    finish(response, started, tls_warning)
}

/// Runs one request on `agent`.
fn run(
    agent: &ureq::Agent,
    request: &ureq::http::Request<Vec<u8>>,
) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    agent.run(request.clone())
}

/// Reads the whole response body and turns it into a [`Response`].
fn finish(
    mut response: ureq::http::Response<ureq::Body>,
    started: Instant,
    tls_warning: Option<String>,
) -> Result<Response, HttpError> {
    let status = response.status().as_u16();
    let headers = collect_headers(response.headers());
    let body = response.body_mut().read_to_vec().map_err(map_ureq_error)?;
    let time = started.elapsed();
    let size = body.len();

    Ok(Response {
        status,
        headers,
        body,
        time,
        size,
        tls_warning,
    })
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
/// TLS verification settings.
fn build_agent(options: &SendOptions, verify_tls: bool) -> ureq::Agent {
    let tls_config = ureq::tls::TlsConfig::builder()
        .disable_verification(!verify_tls)
        .build();

    let max_redirects = if options.follow_redirects {
        options.max_redirects
    } else {
        0
    };

    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .allow_non_standard_methods(true)
        .max_redirects(max_redirects)
        .timeout_global(Some(options.timeout))
        .tls_config(tls_config)
        .build();

    config.new_agent()
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
