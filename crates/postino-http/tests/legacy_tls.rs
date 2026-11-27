//! Servers that only offer TLS rustls does not implement: the request is sent again with the
//! system TLS library. The test server is OpenSSL, which still speaks it, so Linux only.

#![cfg(all(unix, not(target_vendor = "apple")))]
// `expect()` is the normal way to fail a test with a clear message; allowed crate-wide since
// this whole file is test code.
#![allow(clippy::expect_used)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

use openssl::asn1::Asn1Time;
use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::rsa::Rsa;
use openssl::ssl::{SslAcceptor, SslMethod, SslVersion};
use openssl::x509::{X509, X509NameBuilder};
use postino_core::{Method, ResolvedRequest};
use postino_http::{Client, HttpError, InvalidCertificates, SendOptions};

fn request_to(url: String) -> ResolvedRequest {
    ResolvedRequest {
        method: Method::Get,
        url,
        ..ResolvedRequest::default()
    }
}

/// An HTTPS server that offers only `AES128-SHA` (RSA key exchange, CBC) over TLS 1.2, with a
/// self-signed RSA certificate, like the server of the bug report. It accepts `connections`
/// connections, failed handshakes included, answers each request it reads with a 200 and the
/// body `legacy`, and sends the request line of each one.
fn start_legacy_server(
    connections: usize,
) -> (String, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
    let key = PKey::from_rsa(Rsa::generate(2048).expect("rsa key")).expect("pkey");
    let mut name = X509NameBuilder::new().expect("name");
    name.append_entry_by_text("CN", "legacy").expect("cn");
    let name = name.build();
    let mut cert = X509::builder().expect("x509");
    cert.set_version(2).expect("version");
    cert.set_subject_name(&name).expect("subject");
    cert.set_issuer_name(&name).expect("issuer");
    cert.set_pubkey(&key).expect("pubkey");
    cert.set_not_before(&Asn1Time::days_from_now(0).expect("time"))
        .expect("not before");
    cert.set_not_after(&Asn1Time::days_from_now(1).expect("time"))
        .expect("not after");
    cert.sign(&key, MessageDigest::sha256()).expect("sign");
    let cert = cert.build();

    let mut acceptor = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls()).expect("acceptor");
    acceptor.set_private_key(&key).expect("key");
    acceptor.set_certificate(&cert).expect("certificate");
    acceptor
        .set_max_proto_version(Some(SslVersion::TLS1_2))
        .expect("max version");
    acceptor
        .set_cipher_list("AES128-SHA:@SECLEVEL=0")
        .expect("cipher list");
    let acceptor = acceptor.build();

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("https://{}", listener.local_addr().expect("local addr"));
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        for _ in 0..connections {
            let Ok((tcp, _)) = listener.accept() else {
                return;
            };
            // Fails for rustls and for a client that rejects the certificate, as expected.
            let Ok(mut stream) = acceptor.accept(tcp) else {
                continue;
            };
            let mut buffer = [0_u8; 4096];
            let Ok(read) = stream.read(&mut buffer) else {
                continue;
            };
            let text = String::from_utf8_lossy(&buffer[..read]);
            let _ = tx.send(text.lines().next().unwrap_or_default().to_string());
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nlegacy",
            );
            let _ = stream.shutdown();
        }
    });
    (url, rx, handle)
}

#[test]
fn a_legacy_server_is_sent_to_with_the_system_tls_library() {
    // rustls, then the system library.
    let (url, requests, handle) = start_legacy_server(2);
    let options = SendOptions {
        invalid_certificates: InvalidCertificates::Accept,
        ..SendOptions::default()
    };

    let response = postino_http::send(&request_to(format!("{url}/old")), &options).expect("sent");

    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"legacy");
    assert!(response.legacy_tls);
    assert_eq!(response.tls_warning, None);
    assert_eq!(requests.recv().expect("one request"), "GET /old HTTP/1.1");
    handle.join().expect("server thread");
}

#[test]
fn a_legacy_server_with_an_untrusted_certificate_is_sent_to_with_a_warning() {
    // rustls, the system library verifying, then without verification.
    let (url, requests, handle) = start_legacy_server(3);

    let response =
        postino_http::send(&request_to(format!("{url}/")), &SendOptions::default()).expect("sent");

    assert!(response.legacy_tls);
    let warning = response.tls_warning.expect("a warning is set");
    assert!(warning.contains("self-signed"), "{warning}");
    // The server read the request once: the failed handshakes sent nothing.
    assert_eq!(requests.iter().count(), 1);
    handle.join().expect("server thread");
}

#[test]
fn a_client_remembers_a_legacy_server() {
    // Three connections for the first request, one for the second.
    let (url, requests, handle) = start_legacy_server(4);
    let client = Client::new(SendOptions::default());

    for _ in 0..2 {
        let response = client.send(&request_to(format!("{url}/"))).expect("sent");
        assert_eq!(response.body, b"legacy");
        assert!(response.legacy_tls);
        assert!(response.tls_warning.is_some());
    }

    assert_eq!(requests.iter().count(), 2);
    handle.join().expect("server thread");
}

#[test]
fn reject_fails_on_a_legacy_server_with_an_untrusted_certificate() {
    // rustls, then the system library verifying.
    let (url, requests, handle) = start_legacy_server(2);
    let options = SendOptions {
        invalid_certificates: InvalidCertificates::Reject,
        ..SendOptions::default()
    };

    let result = postino_http::send(&request_to(format!("{url}/")), &options);

    assert!(matches!(result, Err(HttpError::Network(_))), "{result:?}");
    handle.join().expect("server thread");
    assert_eq!(requests.iter().count(), 0);
}
