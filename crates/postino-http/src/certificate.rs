//! A small reader of X.509 certificates in DER, enough for `--verbose` (#299): the subject, the
//! issuer, the validity and the subject alternative names. Anything it does not understand is
//! left out, never an error: the certificate was already verified (or deliberately not) by
//! rustls, this only describes it.

use std::net::{Ipv4Addr, Ipv6Addr};

/// What `--verbose` shows of one certificate of the chain the server sent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CertificateInfo {
    /// The subject, such as `CN=example.com, O=Example`.
    pub subject: String,
    /// The issuer, in the same form.
    pub issuer: String,
    /// The start of the validity, such as `2026-01-01 00:00:00 UTC`.
    pub not_before: String,
    /// The end of the validity, in the same form.
    pub not_after: String,
    /// The DNS names and IP addresses of the subject alternative name extension.
    pub alt_names: Vec<String>,
}

/// Describes the DER certificate `der`. `None` when it is not a certificate this reader can
/// walk.
pub(crate) fn describe(der: &[u8]) -> Option<CertificateInfo> {
    let (tag, certificate, _) = tlv(der)?;
    expect(tag, SEQUENCE)?;
    let (tag, tbs, _) = tlv(certificate)?;
    expect(tag, SEQUENCE)?;

    let (mut tag, _, mut next) = tlv(tbs)?;
    if tag == VERSION {
        (tag, _, next) = tlv(next)?;
    }
    // The serial number, then the signature algorithm.
    expect(tag, INTEGER)?;
    let (_, _, after_algorithm) = tlv(next)?;
    let (tag, issuer, after_issuer) = tlv(after_algorithm)?;
    expect(tag, SEQUENCE)?;
    let (tag, validity, after_validity) = tlv(after_issuer)?;
    expect(tag, SEQUENCE)?;
    let (tag, subject, after_subject) = tlv(after_validity)?;
    expect(tag, SEQUENCE)?;

    let (_, not_before, after_before) = tlv(validity)?;
    let (_, not_after, _) = tlv(after_before)?;

    let mut info = CertificateInfo {
        subject: name(subject),
        issuer: name(issuer),
        not_before: time(not_before),
        not_after: time(not_after),
        alt_names: Vec::new(),
    };
    // The public key, then the optional unique ids and extensions.
    let (_, _, mut rest) = tlv(after_subject)?;
    while let Some((tag, content, next)) = tlv(rest) {
        if tag == EXTENSIONS {
            info.alt_names = alt_names(content).unwrap_or_default();
        }
        rest = next;
    }
    Some(info)
}

const INTEGER: u8 = 0x02;
const OCTET_STRING: u8 = 0x04;
const OID: u8 = 0x06;
const SEQUENCE: u8 = 0x30;
const SET: u8 = 0x31;
const VERSION: u8 = 0xa0;
const EXTENSIONS: u8 = 0xa3;
const DNS_NAME: u8 = 0x82;
const IP_ADDRESS: u8 = 0x87;
/// 2.5.29.17, subject alternative name.
const SUBJECT_ALT_NAME: &[u8] = &[0x55, 0x1d, 0x11];

fn expect(tag: u8, wanted: u8) -> Option<()> {
    (tag == wanted).then_some(())
}

/// Splits the first tag, length and value of `input`: the tag, the content and what follows.
fn tlv(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (length, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let length = rest[..count]
            .iter()
            .fold(0usize, |length, &byte| (length << 8) | usize::from(byte));
        (length, &rest[count..])
    };
    if rest.len() < length {
        return None;
    }
    Some((tag, &rest[..length], &rest[length..]))
}

/// A distinguished name as `C=US, O=Example, CN=example.com`, in the certificate's order.
fn name(mut input: &[u8]) -> String {
    let mut parts = Vec::new();
    while let Some((tag, set, next)) = tlv(input) {
        input = next;
        if tag != SET {
            continue;
        }
        let mut attributes = set;
        while let Some((_, attribute, next)) = tlv(attributes) {
            attributes = next;
            let Some((OID, oid, value)) = tlv(attribute) else {
                continue;
            };
            let (Some(label), Some((_, text, _))) = (attribute_label(oid), tlv(value)) else {
                continue;
            };
            parts.push(format!("{label}={}", String::from_utf8_lossy(text)));
        }
    }
    parts.join(", ")
}

/// The short label of the attribute types a name usually has.
fn attribute_label(oid: &[u8]) -> Option<&'static str> {
    match oid {
        [0x55, 0x04, 0x03] => Some("CN"),
        [0x55, 0x04, 0x06] => Some("C"),
        [0x55, 0x04, 0x07] => Some("L"),
        [0x55, 0x04, 0x08] => Some("ST"),
        [0x55, 0x04, 0x0a] => Some("O"),
        [0x55, 0x04, 0x0b] => Some("OU"),
        _ => None,
    }
}

/// A `UTCTime` (`YYMMDDHHMMSSZ`) or `GeneralizedTime` (`YYYYMMDDHHMMSSZ`) as
/// `YYYY-MM-DD HH:MM:SS UTC`. Anything else is shown as written.
fn time(input: &[u8]) -> String {
    let text = String::from_utf8_lossy(input);
    let digits = text.trim_end_matches('Z');
    let full = match digits.len() {
        12 => {
            let year: u32 = digits[..2].parse().unwrap_or(0);
            let century = if year >= 50 { "19" } else { "20" };
            format!("{century}{digits}")
        }
        14 => digits.to_string(),
        _ => return text.into_owned(),
    };
    if !full.bytes().all(|byte| byte.is_ascii_digit()) {
        return text.into_owned();
    }
    format!(
        "{}-{}-{} {}:{}:{} UTC",
        &full[..4],
        &full[4..6],
        &full[6..8],
        &full[8..10],
        &full[10..12],
        &full[12..14]
    )
}

/// The DNS names and IP addresses of the subject alternative name extension, if there is one.
fn alt_names(extensions: &[u8]) -> Option<Vec<String>> {
    let (_, mut list, _) = tlv(extensions)?;
    while let Some((_, extension, next)) = tlv(list) {
        list = next;
        let Some((OID, oid, mut rest)) = tlv(extension) else {
            continue;
        };
        if oid != SUBJECT_ALT_NAME {
            continue;
        }
        // An optional `critical` boolean comes before the value.
        let value = loop {
            let (tag, content, next) = tlv(rest)?;
            if tag == OCTET_STRING {
                break content;
            }
            rest = next;
        };
        let (_, mut names, _) = tlv(value)?;
        let mut found = Vec::new();
        while let Some((tag, content, next)) = tlv(names) {
            names = next;
            match (tag, content.len()) {
                (DNS_NAME, _) => found.push(String::from_utf8_lossy(content).into_owned()),
                (IP_ADDRESS, 4) => {
                    let octets: [u8; 4] = content.try_into().ok()?;
                    found.push(Ipv4Addr::from(octets).to_string());
                }
                (IP_ADDRESS, 16) => {
                    let octets: [u8; 16] = content.try_into().ok()?;
                    found.push(Ipv6Addr::from(octets).to_string());
                }
                _ => {}
            }
        }
        return Some(found);
    }
    None
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn times_read_both_forms() {
        assert_eq!(time(b"260101120000Z"), "2026-01-01 12:00:00 UTC");
        assert_eq!(time(b"991231235959Z"), "1999-12-31 23:59:59 UTC");
        assert_eq!(time(b"20500101000000Z"), "2050-01-01 00:00:00 UTC");
        assert_eq!(time(b"garbage"), "garbage");
    }

    #[test]
    fn long_lengths_and_truncated_input() {
        let mut long = vec![0x04, 0x81, 0x80];
        long.extend([7u8; 0x80]);
        let (tag, content, rest) = tlv(&long).expect("tlv");
        assert_eq!((tag, content.len(), rest.len()), (0x04, 0x80, 0));
        assert_eq!(tlv(&[0x04, 0x05, 1, 2]), None);
        assert_eq!(describe(&[0x30, 0x01]), None);
    }
}
