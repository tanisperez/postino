//! `-v/--verbose` (#299): what happens on the wire, to stderr, in the style of `curl -v`.
//!
//! Lines starting with `*` tell the steps as they happen (the addresses a host resolved to, the
//! connection, the TLS handshake with the server's certificate, each redirect), with the time
//! since the request started. Once the response is in, `>` lines show what was sent and `<`
//! lines what came back. Secrets stay out: the URLs have no query string and sensitive header
//! values are masked, as in the log.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use postino_core::log_safe;
use postino_http::{TlsInfo, Trace, TraceEvent};
use postino_runner::RunResult;

use crate::outcome::{format_duration, format_size};
use crate::style::Style;

/// The clock of the request being sent, shared with the trace callback.
#[derive(Debug)]
struct Clock {
    started: Instant,
    connected: bool,
}

/// Prints the trace of each request; [`Self::start`] before sending one.
#[derive(Debug, Clone)]
pub(crate) struct Verbose {
    clock: Arc<Mutex<Clock>>,
    style: Style,
}

impl Verbose {
    /// A verbose printer with `style` (stderr's), and the trace to put in the send options.
    pub(crate) fn new(style: Style) -> (Self, Trace) {
        let clock = Arc::new(Mutex::new(Clock {
            started: Instant::now(),
            connected: false,
        }));
        let verbose = Self { clock, style };
        let printer = verbose.clone();
        let trace = Trace::new(move |event| printer.event(event));
        (verbose, trace)
    }

    /// Restarts the clock for a new request.
    pub(crate) fn start(&self) {
        let mut clock = self.clock();
        clock.started = Instant::now();
        clock.connected = false;
    }

    fn clock(&self) -> std::sync::MutexGuard<'_, Clock> {
        self.clock.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn event(&self, event: &TraceEvent) {
        let elapsed = {
            let mut clock = self.clock();
            if matches!(event, TraceEvent::Connected { .. }) {
                clock.connected = true;
            }
            clock.started.elapsed()
        };
        let lines = event_lines(event);
        let mut text = String::new();
        for (index, line) in lines.iter().enumerate() {
            let line = if index == 0 {
                format!("* {line} ({})", format_duration(elapsed))
            } else {
                format!("* {line}")
            };
            text.push_str(&self.style.dim(&line));
            text.push('\n');
        }
        eprint!("{text}");
    }

    /// The `>` and `<` lines of `result`, once it is in.
    pub(crate) fn exchange(&self, result: &RunResult) -> String {
        let mut out = String::new();
        let Some(request) = &result.resolved_request else {
            return out;
        };
        if result.response.is_some() && !self.clock().connected {
            out.push_str(&self.style.dim("* Reusing the open connection"));
            out.push('\n');
        }
        out.push_str(&format!(
            "> {} {}\n",
            request.method,
            log_safe::url_for_log(&request.url)
        ));
        for header in &request.headers {
            out.push_str(&format!(
                "> {}: {}\n",
                header.name,
                log_safe::header_value_for_log(&header.name, &header.value)
            ));
        }
        out.push_str(">\n");
        if let Some(response) = &result.response {
            out.push_str(&format!("< HTTP {}\n", response.status));
            for header in &response.headers {
                out.push_str(&format!(
                    "< {}: {}\n",
                    header.name,
                    log_safe::header_value_for_log(&header.name, &header.value)
                ));
            }
            out.push_str("<\n");
            let line = format!(
                "* {} received in {}",
                format_size(response.size as u64),
                format_duration(response.time)
            );
            out.push_str(&self.style.dim(&line));
            out.push('\n');
        }
        out
    }
}

/// The lines of one event: the first gets the time.
fn event_lines(event: &TraceEvent) -> Vec<String> {
    match event {
        TraceEvent::Resolved {
            host,
            port,
            addresses,
        } => {
            let addresses = addresses
                .iter()
                .map(|address| address.ip().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            vec![format!("Host {host}:{port} is {addresses}")]
        }
        TraceEvent::Connected { host, port } => vec![format!("Connected to {host} port {port}")],
        TraceEvent::Tls(tls) => tls_lines(tls),
        TraceEvent::TlsFailed(reason) => vec![format!("TLS handshake failed: {reason}")],
        TraceEvent::Redirected(url) => vec![format!("Redirected to {url}")],
    }
}

/// How many alternative names of a certificate are listed; a CDN's can have hundreds.
const MAX_ALT_NAMES: usize = 8;

fn tls_lines(tls: &TlsInfo) -> Vec<String> {
    let mut handshake = format!("TLS handshake done: {}, {}", tls.version, tls.cipher);
    if let Some(alpn) = &tls.alpn {
        handshake.push_str(&format!(", ALPN {alpn}"));
    }
    let mut lines = vec![handshake];
    if let Some(certificate) = tls.certificates.first() {
        lines.push("Server certificate:".to_string());
        lines.push(format!("  subject: {}", certificate.subject));
        lines.push(format!("  issuer: {}", certificate.issuer));
        lines.push(format!(
            "  valid from {} until {}",
            certificate.not_before, certificate.not_after
        ));
        if !certificate.alt_names.is_empty() {
            let shown = certificate.alt_names.len().min(MAX_ALT_NAMES);
            let mut names = certificate.alt_names[..shown].join(", ");
            let more = certificate.alt_names.len() - shown;
            if more > 0 {
                names.push_str(&format!(" and {more} more"));
            }
            lines.push(format!("  alternative names: {names}"));
        }
    }
    let chain = match tls.certificates.len() {
        0 | 1 => String::new(),
        count => format!(", chain of {count} certificates"),
    };
    lines.push(if tls.verified {
        format!("  verified{chain}")
    } else {
        format!("  NOT verified{chain}")
    });
    lines
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use postino_http::CertificateInfo;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_handshake_tells_the_certificate_and_whether_it_was_verified() {
        let tls = TlsInfo {
            version: "TLSv1.3".to_string(),
            cipher: "TLS13_AES_256_GCM_SHA384".to_string(),
            alpn: None,
            verified: false,
            certificates: vec![
                CertificateInfo {
                    subject: "CN=example.com".to_string(),
                    issuer: "CN=Example CA".to_string(),
                    not_before: "2026-01-01 00:00:00 UTC".to_string(),
                    not_after: "2027-01-01 00:00:00 UTC".to_string(),
                    alt_names: vec!["example.com".to_string(), "127.0.0.1".to_string()],
                },
                CertificateInfo::default(),
            ],
        };
        assert_eq!(
            tls_lines(&tls),
            vec![
                "TLS handshake done: TLSv1.3, TLS13_AES_256_GCM_SHA384",
                "Server certificate:",
                "  subject: CN=example.com",
                "  issuer: CN=Example CA",
                "  valid from 2026-01-01 00:00:00 UTC until 2027-01-01 00:00:00 UTC",
                "  alternative names: example.com, 127.0.0.1",
                "  NOT verified, chain of 2 certificates",
            ]
        );
    }

    #[test]
    fn a_resolution_lists_the_addresses() {
        let event = TraceEvent::Resolved {
            host: "localhost".to_string(),
            port: 8080,
            addresses: vec![
                "[::1]:8080".parse().expect("v6"),
                "127.0.0.1:8080".parse().expect("v4"),
            ],
        };
        assert_eq!(
            event_lines(&event),
            vec!["Host localhost:8080 is ::1, 127.0.0.1"]
        );
    }
}
