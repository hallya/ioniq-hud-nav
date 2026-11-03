//! Standard base64 (RFC 4648 §4, with padding), for the Bluelink `Stamp`
//! header, and the base64url decoding of JWK values (§5). Small enough not to
//! justify a dependency.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes padded or unpadded standard base64; `None` on any other character.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let data = text.trim_end_matches('=');
    if text.len() - data.len() > 2 || data.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(data.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for c in data.bytes() {
        let value = ALPHABET.iter().position(|&a| a == c)? as u32;
        acc = acc << 6 | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Decodes base64url (RFC 4648 §5), padded or not; `None` on `+`, `/` or
/// any other character outside the URL-safe alphabet.
pub fn decode_url(text: &str) -> Option<Vec<u8>> {
    if text.contains(['+', '/']) {
        return None;
    }
    decode(&text.replace('-', "+").replace('_', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RFC4648: [(&str, &str); 7] = [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ];

    #[test]
    fn encodes_rfc_vectors() {
        for (plain, coded) in RFC4648 {
            assert_eq!(encode(plain.as_bytes()), coded, "{plain}");
        }
    }

    #[test]
    fn decodes_rfc_vectors() {
        for (plain, coded) in RFC4648 {
            assert_eq!(decode(coded).as_deref(), Some(plain.as_bytes()), "{coded}");
        }
    }

    #[test]
    fn round_trips_every_byte() {
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&all)), Some(all));
    }

    #[test]
    fn accepts_missing_padding_and_rejects_junk() {
        assert_eq!(decode("Zg").as_deref(), Some(&b"f"[..]));
        assert_eq!(decode("Zm9vYg").as_deref(), Some(&b"foob"[..]));
        assert_eq!(decode("Zm9v!"), None);
        assert_eq!(decode("Z"), None);
        assert_eq!(decode("Zg=a"), None);
    }

    #[test]
    fn decodes_url_safe_alphabet_only() {
        assert_eq!(decode_url("-_8").as_deref(), Some(&[0xfb, 0xff][..]));
        assert_eq!(decode_url("AQAB").as_deref(), Some(&[1, 0, 1][..]));
        assert_eq!(decode_url("Zg==").as_deref(), Some(&b"f"[..]));
        assert_eq!(decode_url(""), Some(Vec::new()));
        assert_eq!(decode_url("+/8"), None);
        assert_eq!(decode_url("Zm9v!"), None);
    }
}
