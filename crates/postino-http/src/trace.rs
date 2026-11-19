//! Following a request on the wire, for the command line's `--verbose` (#299): the name
//! resolution, the connection, the TLS handshake with the server's certificates, and the
//! redirects followed.
//!
//! A [`Trace`] in [`crate::SendOptions::trace`] gives the client a connector chain of its own,
//! the one `ureq` builds by default with two observers around the TCP connection and its own
//! TLS step instead of `ureq`'s: `ureq` keeps its TLS session private, and the negotiated
//! version, cipher suite and certificates are only known inside the handshake. That TLS step
//! is `ureq`'s `RustlsConnector` (MIT or Apache-2.0) with the configuration built from Postino's
//! options and the details read once the handshake is done. Without a trace nothing of this
//! runs: the app and the load tests use `ureq`'s default connector.

use std::fmt;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, RootCertStore, StreamOwned};
use ureq::tls::RootCerts;
use ureq::unversioned::transport::{
    Buffers, ConnectProxyConnector, ConnectionDetails, Connector, Either, LazyBuffers, NextTimeout,
    TcpConnector, Transport, TransportAdapter,
};

pub use crate::certificate::CertificateInfo;

/// A step of sending a request, in the order they happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceEvent {
    /// The host is about to be connected to, at these addresses (tried in order).
    Resolved {
        /// The host name of the URL, or of the proxy.
        host: String,
        /// The port.
        port: u16,
        /// The addresses it resolved to.
        addresses: Vec<SocketAddr>,
    },
    /// A TCP connection to the host is open.
    Connected {
        /// The host name.
        host: String,
        /// The port.
        port: u16,
    },
    /// The TLS handshake is done.
    Tls(TlsInfo),
    /// The TLS handshake failed, for the reason given, such as a rejected certificate.
    TlsFailed(String),
    /// A redirect was followed to this URL.
    Redirected(String),
}

/// What the TLS handshake negotiated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsInfo {
    /// The protocol version, such as `TLSv1.3`.
    pub version: String,
    /// The cipher suite, such as `TLS13_AES_256_GCM_SHA384`.
    pub cipher: String,
    /// The application protocol agreed with ALPN, if any.
    pub alpn: Option<String>,
    /// Whether the server's certificate was verified. `false` when verification is off, for
    /// `--insecure` or the retry after a rejected certificate.
    pub verified: bool,
    /// The certificates the server sent, its own first.
    pub certificates: Vec<CertificateInfo>,
}

/// Receives the [`TraceEvent`]s of every request sent with the options that hold it. Called on
/// the thread that sends.
#[derive(Clone)]
pub struct Trace(Arc<dyn Fn(&TraceEvent) + Send + Sync>);

impl Trace {
    /// A trace that hands every event to `on_event`.
    pub fn new(on_event: impl Fn(&TraceEvent) + Send + Sync + 'static) -> Self {
        Self(Arc::new(on_event))
    }

    pub(crate) fn emit(&self, event: &TraceEvent) {
        (self.0)(event);
    }
}

impl fmt::Debug for Trace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Trace")
    }
}

/// Two traces are equal when they are the same callback.
impl PartialEq for Trace {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Trace {}

/// The connector chain of a traced agent: `ureq`'s default one for HTTP/1.1 over TCP, with a
/// CONNECT proxy if set, plus the observers and the traced TLS step.
pub(crate) fn connector(trace: Trace, roots: RootCerts, verify: bool) -> impl Connector {
    ().chain(Resolving(trace.clone()))
        .chain(ConnectProxyConnector::default())
        .chain(TcpConnector::default())
        .chain(Connecting(trace.clone()))
        .chain(TracedTls {
            trace,
            roots,
            verify,
            config: OnceLock::new(),
        })
}

/// The host and port a connection goes to: the URL's, or the proxy's.
fn host_port(details: &ConnectionDetails) -> (String, u16) {
    let host = details.uri.host().unwrap_or_default().to_string();
    let port = details
        .uri
        .port_u16()
        .unwrap_or(if details.uri.scheme_str() == Some("https") {
            443
        } else {
            80
        });
    (host, port)
}

/// Tells where a new connection goes, before it is opened.
#[derive(Debug)]
struct Resolving(Trace);

impl<In: Transport> Connector<In> for Resolving {
    type Out = In;

    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        if chained.is_none() {
            let (host, port) = host_port(details);
            self.0.emit(&TraceEvent::Resolved {
                host,
                port,
                addresses: details.addrs.iter().copied().collect(),
            });
        }
        Ok(chained)
    }
}

/// Tells that the TCP connection is open.
#[derive(Debug)]
struct Connecting(Trace);

impl<In: Transport> Connector<In> for Connecting {
    type Out = In;

    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        if let Some(transport) = &chained
            && !transport.is_tls()
        {
            let (host, port) = host_port(details);
            self.0.emit(&TraceEvent::Connected { host, port });
        }
        Ok(chained)
    }
}

/// The TLS step: `ureq`'s `RustlsConnector`, telling what the handshake negotiated.
struct TracedTls {
    trace: Trace,
    roots: RootCerts,
    verify: bool,
    config: OnceLock<Arc<ClientConfig>>,
}

impl fmt::Debug for TracedTls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TracedTls")
            .field("verify", &self.verify)
            .finish_non_exhaustive()
    }
}

impl<In: Transport> Connector<In> for TracedTls {
    type Out = Either<In, TlsTransport>;

    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        let Some(transport) = chained else {
            return Ok(None);
        };
        if !details.needs_tls() || transport.is_tls() {
            return Ok(Some(Either::A(transport)));
        }
        let config = self.config()?;
        let host = details.uri.host().unwrap_or_default();
        let host = host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(host);
        let name = ServerName::try_from(host.to_string())
            .map_err(|_| ureq::Error::Tls("Rustls invalid dns name error"))?;

        let mut connection = ClientConnection::new(config, name)?;
        let mut socket = TransportAdapter::new(transport.boxed(), details.current_time.clone());
        socket.set_timeout(details.timeout);
        if let Err(error) = connection.complete_io(&mut socket) {
            let reason = error
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<rustls::Error>())
                .map_or_else(|| error.to_string(), ToString::to_string);
            self.trace.emit(&TraceEvent::TlsFailed(reason));
            return Err(error.into());
        }
        self.trace
            .emit(&TraceEvent::Tls(tls_info(&connection, self.verify)));

        let buffers = LazyBuffers::new(
            details.config.input_buffer_size(),
            details.config.output_buffer_size(),
        );
        Ok(Some(Either::B(TlsTransport {
            buffers,
            stream: StreamOwned {
                conn: connection,
                sock: socket,
            },
        })))
    }
}

impl TracedTls {
    /// The rustls configuration, built on the first connection: the roots of the options, or no
    /// verification at all.
    fn config(&self) -> Result<Arc<ClientConfig>, ureq::Error> {
        if let Some(config) = self.config.get() {
            return Ok(config.clone());
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let builder = ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(rustls::ALL_VERSIONS)?;
        let builder = if !self.verify {
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoVerification(provider)))
        } else {
            match &self.roots {
                RootCerts::Specific(certificates) => {
                    let mut store = RootCertStore::empty();
                    store.add_parsable_certificates(
                        certificates
                            .iter()
                            .map(|certificate| CertificateDer::from(certificate.der())),
                    );
                    builder.with_root_certificates(store)
                }
                RootCerts::PlatformVerifier => builder
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(
                        rustls_platform_verifier::Verifier::new(provider)?,
                    )),
                _ => builder.with_root_certificates(RootCertStore {
                    roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
                }),
            }
        };
        let config = Arc::new(builder.with_no_client_auth());
        Ok(self.config.get_or_init(|| config).clone())
    }
}

/// What the handshake of `connection` negotiated.
fn tls_info(connection: &ClientConnection, verified: bool) -> TlsInfo {
    let version = connection
        .protocol_version()
        .map(|version| match version {
            rustls::ProtocolVersion::TLSv1_3 => "TLSv1.3".to_string(),
            rustls::ProtocolVersion::TLSv1_2 => "TLSv1.2".to_string(),
            other => format!("{other:?}"),
        })
        .unwrap_or_default();
    let cipher = connection
        .negotiated_cipher_suite()
        .map(|suite| format!("{:?}", suite.suite()))
        .unwrap_or_default();
    TlsInfo {
        version,
        cipher,
        alpn: connection
            .alpn_protocol()
            .map(|protocol| String::from_utf8_lossy(protocol).into_owned()),
        verified,
        certificates: connection
            .peer_certificates()
            .unwrap_or_default()
            .iter()
            .filter_map(|certificate| crate::certificate::describe(certificate))
            .collect(),
    }
}

/// `ureq`'s `RustlsTransport`.
pub(crate) struct TlsTransport {
    buffers: LazyBuffers,
    stream: StreamOwned<ClientConnection, TransportAdapter>,
}

impl fmt::Debug for TlsTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TlsTransport")
    }
}

impl Transport for TlsTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.stream.get_mut().set_timeout(timeout);
        let output = &self.buffers.output()[..amount];
        self.stream.write_all(output)?;
        Ok(())
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        self.stream.get_mut().set_timeout(timeout);
        let input = self.buffers.input_append_buf();
        let amount = self.stream.read(input)?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        self.stream.get_mut().get_mut().is_open()
    }

    fn is_tls(&self) -> bool {
        true
    }
}

/// Accepts any certificate, for `--insecure` and the retry after a rejected one. The
/// signatures of the handshake are still checked.
#[derive(Debug)]
struct NoVerification(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for NoVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            certificate,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            certificate,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
