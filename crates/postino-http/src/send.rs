//! The `send` function: hands a [`ResolvedRequest`] to `ureq` and turns the result back into
//! plain `postino-core` types.

use std::time::Instant;

use postino_core::{ResolvedBody, ResolvedField, ResolvedRequest, Response};

use crate::error::HttpError;
use crate::options::SendOptions;

/// Sends a fully resolved request over HTTP and returns the response.
///
/// 4xx and 5xx status codes are returned as a normal [`Response`], not as an [`HttpError`]:
/// only failures that prevent us from getting any response at all (an invalid request, a
/// timeout, too many redirects, a connection failure, ...) are errors. The returned response's
/// `time` is the total time spent sending the request and reading the whole response body.
pub fn send(request: &ResolvedRequest, options: &SendOptions) -> Result<Response, HttpError> {
    let agent = build_agent(options);
    let http_request = build_http_request(request)?;

    let started = Instant::now();
    let mut response = agent.run(http_request).map_err(map_ureq_error)?;

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
    })
}

/// Builds a `ureq` agent configured from `options`.
///
/// This disables `ureq`'s default behavior of treating 4xx/5xx as errors, allows the
/// non-standard HTTP methods a `.postino` file may use, and applies the timeout, redirect and
/// TLS verification settings.
fn build_agent(options: &SendOptions) -> ureq::Agent {
    let tls_config = ureq::tls::TlsConfig::builder()
        .disable_verification(!options.verify_tls)
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
        other => HttpError::Network(other),
    }
}
