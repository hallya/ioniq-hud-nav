//! `X-Fingerprint` signature required by the EU CCI auth endpoints since 2026-10-14.
//!
//! HMAC-SHA256, hex encoded:
//! - key: identity + secret + piece + suffix
//! - message: identity + `client-os-version` + `X-Timestamp` + body without ASCII whitespace
//!
//! where identity = `client-id` + `client-name` + `client-version` + `client-os-code`.
//! Header names are matched case-insensitively. Algorithm from upstream
//! `hyundai_kia_connect_api/cci_fingerprint.py` (MIT).
//!
//! The three key constants come from the official app and may change with its
//! releases, so they are data ([`SigningKey`]), never hard-coded here.

/// App-embedded constants concatenated into the HMAC key, in this order.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SigningKey {
    pub secret: String,
    pub piece: String,
    pub suffix: String,
}

use hmac::{Hmac, Mac};
use sha2::Sha256;

const IDENTITY_HEADERS: [&str; 4] = [
    "client-id",
    "client-name",
    "client-version",
    "client-os-code",
];

/// Lowercase hex HMAC-SHA256 over the request identity, timestamp and body.
pub fn x_fingerprint(headers: &[(String, String)], body: &str, key: &SigningKey) -> String {
    let identity: String = IDENTITY_HEADERS
        .iter()
        .map(|name| header(headers, name))
        .collect();
    let hmac_key = [identity.as_str(), &key.secret, &key.piece, &key.suffix].concat();
    let compact_body: String = body.chars().filter(|c| !is_ascii_space(*c)).collect();

    let mut mac =
        Hmac::<Sha256>::new_from_slice(hmac_key.as_bytes()).expect("HMAC takes keys of any length");
    mac.update(identity.as_bytes());
    mac.update(header(headers, "client-os-version").as_bytes());
    mac.update(header(headers, "x-timestamp").as_bytes());
    mac.update(compact_body.as_bytes());
    to_hex(&mac.finalize().into_bytes())
}

/// Value of a header, case-insensitive; the last one wins, as in upstream's dict.
fn header<'a>(headers: &'a [(String, String)], name: &str) -> &'a str {
    headers
        .iter()
        .rev()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map_or("", |(_, v)| v.as_str())
}

/// ASCII whitespace as Java and Python's `\s` (ASCII) define it. Unlike
/// `char::is_ascii_whitespace`, it includes the vertical tab.
fn is_ascii_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0B' | '\x0C')
}

fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 0x0F)] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Vectors {
        key: SigningKey,
        headers: Vec<(String, String)>,
        cases: Vec<Case>,
    }

    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        body: String,
        expected: String,
    }

    fn vectors() -> Vectors {
        let raw = include_str!("../../../fixtures/bluelink_eu/fingerprint_vectors.json");
        serde_json::from_str(raw).expect("fingerprint_vectors.json parses")
    }

    #[test]
    fn matches_reference_vectors() {
        let v = vectors();
        assert!(!v.cases.is_empty());
        for case in &v.cases {
            assert_eq!(
                x_fingerprint(&v.headers, &case.body, &v.key),
                case.expected,
                "case: {}",
                case.name
            );
        }
    }

    #[test]
    fn header_order_does_not_matter() {
        let v = vectors();
        let mut reversed = v.headers.clone();
        reversed.reverse();
        let case = &v.cases[1];
        assert_eq!(x_fingerprint(&reversed, &case.body, &v.key), case.expected);
    }

    #[test]
    fn a_different_key_changes_the_signature() {
        let v = vectors();
        let mut other = v.key.clone();
        other.suffix.push('x');
        let body = &v.cases[0].body;
        assert_ne!(
            x_fingerprint(&v.headers, body, &other),
            x_fingerprint(&v.headers, body, &v.key)
        );
    }
}
