//! Signature repair from upstream's `cci_fingerprint.py`.
//!
//! When the CCI gate answers `gate9035` or `gate9036`, the app constants are
//! out of date. The app downloads upstream's current
//! `hyundai_kia_connect_api/cci_fingerprint.py` (MIT) and
//! [`FingerprintOverride::from_upstream`] reads only the Hyundai signing key
//! and the Android build identity from it. [`AppConstants::with_override`]
//! applies them; hosts and ids stay as bundled.
//!
//! The file is read as text, never run: a small lexer finds the module-level
//! assignments (`NAME = "value"`) and the `BRAND_HYUNDAI` entry of
//! `_FINGERPRINT_KEYS`. Values go into HTTP headers, so each one must be
//! printable ASCII, without escapes, at most [`MAX_VALUE_LEN`] bytes.

use super::fingerprint::SigningKey;
use super::{AppConstants, Error};

/// Longest value accepted from the downloaded file.
pub const MAX_VALUE_LEN: usize = 128;

/// What a newer upstream file can renew: the HMAC key and the build identity
/// the gate checks it against.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FingerprintOverride {
    pub signing_key: SigningKey,
    pub version: String,
    pub os_code: String,
    pub os_version: String,
    pub device_model: String,
    pub notification_provider: String,
    pub user_agent: String,
    pub region: String,
}

impl FingerprintOverride {
    /// Reads the override from the text of upstream's `cci_fingerprint.py`.
    /// Errors: `parse-fpkey` (no Hyundai key), `parse-fpident` (a build
    /// identity constant is missing), `parse-fpvalue` (a value is unsafe).
    pub fn from_upstream(source: &str) -> Result<Self, Error> {
        let no_key = Error::Parse("fpkey");
        let tokens = lex(source).ok_or(no_key.clone())?;
        let module = assignments(&tokens);

        let keys = module.get("_FINGERPRINT_KEYS").ok_or(no_key.clone())?;
        let [secret, piece, suffix] = hyundai_key(keys).ok_or(no_key)?;
        let signing_key = SigningKey {
            secret: safe(secret)?,
            piece: safe(piece)?,
            suffix: safe(suffix)?,
        };

        let constant = |name: &str| -> Result<String, Error> {
            let value = module.get(name).ok_or(Error::Parse("fpident"))?;
            safe(string_value(value).ok_or(Error::Parse("fpident"))?)
        };
        Ok(FingerprintOverride {
            signing_key,
            version: constant("CLIENT_VERSION")?,
            os_code: constant("CLIENT_OS_CODE")?,
            os_version: constant("CLIENT_OS_VERSION")?,
            device_model: constant("CLIENT_DEVICE_MODEL")?,
            notification_provider: constant("CLIENT_NOTIFICATION_PROVIDER")?,
            user_agent: constant("USER_AGENT")?,
            region: constant("APP_REGION")?,
        })
    }
}

impl AppConstants {
    /// These constants with the signing key and build identity replaced.
    pub fn with_override(&self, o: &FingerprintOverride) -> AppConstants {
        let mut c = self.clone();
        c.signing_key = o.signing_key.clone();
        c.client.version = o.version.clone();
        c.client.os_code = o.os_code.clone();
        c.client.os_version = o.os_version.clone();
        c.client.device_model = o.device_model.clone();
        c.client.notification_provider = o.notification_provider.clone();
        c.client.user_agent = o.user_agent.clone();
        c.client.region = o.region.clone();
        c
    }
}

/// A string literal as written, and whether it can be used as is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Literal<'a> {
    text: &'a str,
    /// No prefix (`r`, `f`, `b`...) and no backslash escape.
    plain: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok<'a> {
    Name(&'a str),
    Str(Literal<'a>),
    Punct(u8),
    /// End of a logical line (a newline outside brackets).
    Newline,
}

#[derive(Debug, Clone, Copy)]
struct Token<'a> {
    tok: Tok<'a>,
    /// First token of its line, at column 0: a module-level statement.
    col0: bool,
}

/// Python-ish tokens, enough to find module-level assignments. `None` when
/// a string is unterminated or brackets do not balance.
fn lex(src: &str) -> Option<Vec<Token<'_>>> {
    let b = src.as_bytes();
    let (mut i, mut depth, mut col) = (0usize, 0i32, 0usize);
    let mut out: Vec<Token> = Vec::new();
    while i < b.len() {
        let c = b[i];
        match c {
            b'\n' => {
                if depth == 0
                    && !matches!(
                        out.last(),
                        None | Some(Token {
                            tok: Tok::Newline,
                            ..
                        })
                    )
                {
                    out.push(Token {
                        tok: Tok::Newline,
                        col0: false,
                    });
                }
                i += 1;
                col = 0;
            }
            b' ' | b'\t' | b'\r' | 0x0c => {
                i += 1;
                col += 1;
            }
            b'#' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'\\' if b.get(i + 1) == Some(&b'\n') => {
                i += 2;
                col = 1; // a continued line is not a new statement
            }
            b'"' | b'\'' => {
                let (lit, end) = string(src, i, true)?;
                push(&mut out, Tok::Str(lit), col);
                col += end - i;
                i = end;
            }
            _ if c == b'_' || c.is_ascii_alphanumeric() || c >= 0x80 => {
                let start = i;
                while i < b.len() && (b[i] == b'_' || b[i].is_ascii_alphanumeric() || b[i] >= 0x80)
                {
                    i += 1;
                }
                let name = &src[start..i];
                let prefix = name.len() <= 2
                    && name.bytes().all(|p| b"rRbBfFuU".contains(&p))
                    && matches!(b.get(i), Some(b'"' | b'\''));
                if prefix {
                    let (mut lit, end) = string(src, i, false)?;
                    lit.plain = false;
                    push(&mut out, Tok::Str(lit), col);
                    col += end - start;
                    i = end;
                } else {
                    push(&mut out, Tok::Name(name), col);
                    col += i - start;
                }
            }
            _ => {
                match c {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => {
                        depth -= 1;
                        if depth < 0 {
                            return None;
                        }
                    }
                    _ => {}
                }
                push(&mut out, Tok::Punct(c), col);
                i += 1;
                col += 1;
            }
        }
    }
    (depth == 0).then_some(out)
}

/// Appends a token; it starts a statement when it opens a logical line at
/// column 0.
fn push<'a>(out: &mut Vec<Token<'a>>, tok: Tok<'a>, col: usize) {
    let line_open = matches!(
        out.last(),
        None | Some(Token {
            tok: Tok::Newline,
            ..
        })
    );
    out.push(Token {
        tok,
        col0: col == 0 && line_open,
    });
}

/// The string literal starting at the quote `src[start]`; returns it and
/// the index just past its closing quote.
fn string(src: &str, start: usize, plain: bool) -> Option<(Literal<'_>, usize)> {
    let b = src.as_bytes();
    let q = b[start];
    let triple = b.get(start..start + 3) == Some(&[q, q, q][..]);
    let open = if triple { 3 } else { 1 };
    let mut i = start + open;
    let mut escaped = false;
    loop {
        match b.get(i)? {
            b'\\' => {
                escaped = true;
                i += 2;
            }
            b'\n' if !triple => return None,
            &c if c == q && (!triple || b.get(i..i + 3) == Some(&[q, q, q][..])) => {
                let lit = Literal {
                    text: &src[start + open..i],
                    plain: plain && !escaped,
                };
                return Some((lit, i + open));
            }
            _ => i += 1,
        }
    }
}

/// Module-level `NAME = value` and `NAME: annotation = value`, value tokens
/// by name; the last assignment wins, as when Python runs the module.
fn assignments<'a>(tokens: &'a [Token<'a>]) -> std::collections::HashMap<&'a str, &'a [Token<'a>]> {
    let mut module = std::collections::HashMap::new();
    for line in tokens.split(|t| t.tok == Tok::Newline) {
        let Some(Token {
            tok: Tok::Name(name),
            col0: true,
        }) = line.first()
        else {
            continue;
        };
        let value_at = match line.get(1).map(|t| t.tok) {
            Some(Tok::Punct(b'=')) => 2,
            Some(Tok::Punct(b':')) => match line.iter().position(|t| t.tok == Tok::Punct(b'=')) {
                Some(eq) => eq + 1,
                None => continue,
            },
            _ => continue,
        };
        if line.get(value_at).map(|t| t.tok) == Some(Tok::Punct(b'=')) {
            continue; // `==`
        }
        module.insert(*name, &line[value_at..]);
    }
    module
}

/// `"a"`, `"a" "b"` or `("a" "b")`: the joined literal. `None` for anything
/// else; a joined literal is plain only if every part is.
fn string_value<'a>(value: &[Token<'a>]) -> Option<(String, bool)> {
    let inner = match value {
        [Token {
            tok: Tok::Punct(b'('),
            ..
        }, inner @ .., Token {
            tok: Tok::Punct(b')'),
            ..
        }] => inner,
        _ => value,
    };
    if inner.is_empty() {
        return None;
    }
    let (mut text, mut plain) = (String::new(), true);
    for t in inner {
        let Tok::Str(lit) = t.tok else { return None };
        text.push_str(lit.text);
        plain &= lit.plain;
    }
    Some((text, plain))
}

/// The three strings of the `BRAND_HYUNDAI: (secret, piece, suffix)` entry.
fn hyundai_key(value: &[Token<'_>]) -> Option<[(String, bool); 3]> {
    let [Token {
        tok: Tok::Punct(b'{'),
        ..
    }, entries @ .., Token {
        tok: Tok::Punct(b'}'),
        ..
    }] = value
    else {
        return None;
    };
    split_top_level(entries)
        .into_iter()
        .find_map(|entry| match entry {
            [Token {
                tok: Tok::Name("BRAND_HYUNDAI"),
                ..
            }, Token {
                tok: Tok::Punct(b':'),
                ..
            }, Token {
                tok: Tok::Punct(b'('),
                ..
            }, tuple @ .., Token {
                tok: Tok::Punct(b')'),
                ..
            }] => Some(tuple),
            _ => None,
        })
        .and_then(|tuple| {
            let parts: Vec<_> = split_top_level(tuple)
                .into_iter()
                .map(string_value)
                .collect();
            match parts.as_slice() {
                [Some(a), Some(b), Some(c)] => Some([a.clone(), b.clone(), c.clone()]),
                _ => None,
            }
        })
}

/// Splits on commas outside brackets; drops empty pieces (trailing comma).
fn split_top_level<'t, 'a>(tokens: &'t [Token<'a>]) -> Vec<&'t [Token<'a>]> {
    let (mut parts, mut depth, mut start) = (Vec::new(), 0i32, 0usize);
    for (i, t) in tokens.iter().enumerate() {
        match t.tok {
            Tok::Punct(b'(' | b'[' | b'{') => depth += 1,
            Tok::Punct(b')' | b']' | b'}') => depth -= 1,
            Tok::Punct(b',') if depth == 0 => {
                parts.push(&tokens[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&tokens[start..]);
    parts.retain(|p| !p.is_empty());
    parts
}

/// A value safe to put in an HTTP header, or `parse-fpvalue`.
fn safe((text, plain): (String, bool)) -> Result<String, Error> {
    let ok = plain
        && !text.is_empty()
        && text.len() <= MAX_VALUE_LEN
        && text.bytes().all(|c| (0x20..=0x7e).contains(&c));
    ok.then_some(text).ok_or(Error::Parse("fpvalue"))
}

#[cfg(test)]
mod tests {
    use super::super::fixtures;
    use super::*;

    fn source() -> String {
        fixtures::text("upstream_synthetic/cci_fingerprint.py")
    }

    fn parse(text: &str) -> Result<FingerprintOverride, String> {
        FingerprintOverride::from_upstream(text).map_err(|e| e.code())
    }

    /// The override the synthetic constants imply: the synthetic upstream
    /// copy holds the same values.
    fn expected() -> FingerprintOverride {
        let c = fixtures::constants();
        FingerprintOverride {
            signing_key: c.signing_key,
            version: c.client.version,
            os_code: c.client.os_code,
            os_version: c.client.os_version,
            device_model: c.client.device_model,
            notification_provider: c.client.notification_provider,
            user_agent: c.client.user_agent,
            region: c.client.region,
        }
    }

    #[test]
    fn reads_hyundai_key_and_build_identity() {
        assert_eq!(parse(&source()), Ok(expected()));
    }

    #[test]
    fn applying_the_matching_upstream_copy_changes_nothing() {
        let c = fixtures::constants();
        let o = FingerprintOverride::from_upstream(&source()).unwrap();
        assert_eq!(c.with_override(&o), c);
    }

    #[test]
    fn override_replaces_only_the_key_and_build_identity() {
        let newer = source()
            .replace("\"TESTsecret01\"", "\"NEWsecret01\"")
            .replace("\"TESTsufx03\"", "\"NEWsufx03\"")
            .replace("\"9.9.9\"", "\"9.9.10\"")
            .replace("\"TEST-MODEL\"", "\"NEW-MODEL\"");
        let c = fixtures::constants();
        let renewed = c.with_override(&FingerprintOverride::from_upstream(&newer).unwrap());

        assert_eq!(renewed.signing_key.secret, "NEWsecret01");
        assert_eq!(renewed.signing_key.piece, "TESTpiece02");
        assert_eq!(renewed.signing_key.suffix, "NEWsufx03");
        assert_eq!(renewed.client.version, "9.9.10");
        assert_eq!(renewed.client.device_model, "NEW-MODEL");

        let mut back = renewed.clone();
        back.signing_key = c.signing_key.clone();
        back.client.version = c.client.version.clone();
        back.client.device_model = c.client.device_model.clone();
        assert_eq!(
            back, c,
            "hosts, ids, package, client name and brand stay bundled"
        );
    }

    #[test]
    fn takes_the_hyundai_entry_wherever_it_sits() {
        let hyundai = "    BRAND_HYUNDAI: (\"TESTsecret01\", \"TESTpiece02\", \"TESTsufx03\"),\n";
        let moved = source().replace(hyundai, "").replace(
            "    BRAND_GENESIS:",
            &format!("{hyundai}    BRAND_GENESIS:"),
        );
        assert_ne!(moved, source());
        assert_eq!(parse(&moved), Ok(expected()));
    }

    #[test]
    fn a_new_brand_entry_does_not_hide_the_hyundai_one() {
        let text = source().replace(
            "    BRAND_KIA: (",
            "    BRAND_NEW: _load_pieces(\"x\", (1, 2)),\n    **_EXTRA,\n    BRAND_KIA: (",
        );
        assert_ne!(text, source());
        assert_eq!(parse(&text), Ok(expected()));
    }

    #[test]
    fn a_missing_hyundai_entry_is_not_replaced_by_another_brand() {
        let without = source().replace(
            "    BRAND_HYUNDAI: (\"TESTsecret01\", \"TESTpiece02\", \"TESTsufx03\"),\n",
            "",
        );
        assert_ne!(without, source());
        assert_eq!(parse(&without), Err("parse-fpkey".into()));
    }

    #[test]
    fn tolerates_reformatting() {
        let reformatted = source()
            .replace(
                "    BRAND_HYUNDAI: (\"TESTsecret01\", \"TESTpiece02\", \"TESTsufx03\"),",
                "    BRAND_HYUNDAI: (  # rotated with app 9.9.9\n        'TESTsecret01',\n        \"TESTpiece02\",\n        'TESTsufx03',\n    ),",
            )
            .replace("CLIENT_VERSION = \"9.9.9\"", "CLIENT_VERSION: str = '9.9.9'  # app")
            .replace(
                "USER_AGENT = \"okhttp/4.12.0\"",
                "USER_AGENT = (\n    \"okhttp/\"\n    \"4.12.0\"\n)",
            );
        assert_eq!(parse(&reformatted), Ok(expected()));
    }

    #[test]
    fn reads_module_level_assignments_only_and_the_last_one_wins() {
        let text = source()
            + "\n\ndef later():\n    CLIENT_VERSION = \"0.0.1\"\n    return \"CLIENT_OS_CODE = 'x'\"\n"
            + "\nCLIENT_DEVICE_MODEL = \"LATER-MODEL\"\n";
        let got = parse(&text).unwrap();
        assert_eq!(got.version, "9.9.9");
        assert_eq!(got.os_code, "android");
        assert_eq!(got.device_model, "LATER-MODEL");
    }

    #[test]
    fn a_missing_identity_constant_fails() {
        let without = source().replace("CLIENT_OS_VERSION = \"14\"\n", "");
        assert_ne!(without, source());
        assert_eq!(parse(&without), Err("parse-fpident".into()));
    }

    #[test]
    fn rejects_values_unsafe_for_headers() {
        let cases = [
            ("\"9.9.9\"", "\"9.9.9\\r\\nX-Evil: 1\""),
            ("\"9.9.9\"", "\"\""),
            ("\"9.9.9\"", "\"9.9.é\""),
            ("\"TESTpiece02\"", "\"TEST\\tpiece\""),
        ];
        for (from, to) in cases {
            let text = source().replacen(from, to, 1);
            assert_eq!(parse(&text), Err("parse-fpvalue".into()), "{to}");
        }
        let long = format!("\"{}\"", "a".repeat(MAX_VALUE_LEN + 1));
        let text = source().replacen("\"android\"", &long, 1);
        assert_eq!(parse(&text), Err("parse-fpvalue".into()));
    }

    #[test]
    fn garbage_and_html_fail_without_panicking() {
        for text in [
            "",
            "<html><body>404: Not Found</body></html>",
            "_FINGERPRINT_KEYS = {\n    BRAND_HYUNDAI: (\"a\", \"b\"",
            "x = \"unterminated",
            "x = '''never closed",
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
    }

    /// Live check against a real upstream file, for the weekly upstream
    /// review: `HUDNAV_UPSTREAM_FP=path/to/cci_fingerprint.py cargo test --
    /// --ignored live_upstream`. Prints lengths only, never values.
    #[test]
    #[ignore]
    fn live_upstream_file_parses() {
        let path = std::env::var("HUDNAV_UPSTREAM_FP").expect("HUDNAV_UPSTREAM_FP");
        let text = std::fs::read_to_string(path).expect("readable");
        let o = FingerprintOverride::from_upstream(&text).expect("parses");
        let k = &o.signing_key;
        println!(
            "key lengths {}/{}/{}, version {} chars, os_code {} chars",
            k.secret.len(),
            k.piece.len(),
            k.suffix.len(),
            o.version.len(),
            o.os_code.len()
        );
        assert_ne!(o, expected(), "real file, not the synthetic copy");

        // With the JSON scripts/app-constants.py wrote from the same commit,
        // the core and the build script must agree.
        if let Ok(path) = std::env::var("HUDNAV_UPSTREAM_CONSTANTS") {
            let c = AppConstants::from_json(&std::fs::read_to_string(path).expect("readable"))
                .expect("bundled constants load");
            assert_eq!(
                c.with_override(&o),
                c,
                "script and core read the same values"
            );
            println!("bundled constants load and agree with the override");
        }
    }

    #[test]
    fn override_round_trips_as_json_for_the_app_to_store() {
        let o = expected();
        let text = serde_json::to_string(&o).unwrap();
        assert_eq!(
            serde_json::from_str::<FingerprintOverride>(&text).unwrap(),
            o
        );
    }
}
