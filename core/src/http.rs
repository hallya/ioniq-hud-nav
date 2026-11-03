//! Plain HTTP values exchanged with the Swift shell.
//!
//! The core produces [`Request`]s and consumes [`Response`]s. The shell sends
//! them with URLSession and hands the bytes back unchanged. The shell keeps one
//! cookie store per login, follows redirects only when
//! [`Request::follow_redirects`] is set, and fills [`Response::url`] with the
//! final URL after redirects.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    /// False when the core needs to read a redirect itself (its `Location`).
    pub follow_redirects: bool,
}

impl Request {
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: None,
            follow_redirects: true,
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers.extend(headers);
        self
    }

    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn no_redirects(mut self) -> Self {
        self.follow_redirects = false;
        self
    }

    /// Value of the first header with this name, compared case-insensitively.
    pub fn header_value(&self, name: &str) -> Option<&str> {
        find_header(&self.headers, name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// Final URL after redirects; empty when unknown.
    pub url: String,
    pub body: String,
}

impl Response {
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            url: String::new(),
            body: body.into(),
        }
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Value of the first header with this name, compared case-insensitively.
    pub fn header_value(&self, name: &str) -> Option<&str> {
        find_header(&self.headers, name)
    }
}

fn find_header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// One value encoded as Python's `quote_plus`: letters, digits and `-_.~`
/// kept, space as `+`, every other byte as `%XX`.
pub fn encode_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push(HEX[usize::from(b >> 4)] as char);
                out.push(HEX[usize::from(b & 0x0F)] as char);
            }
        }
    }
    out
}

/// `application/x-www-form-urlencoded` body or query, in the given order.
pub fn form_encode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", encode_component(k), encode_component(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Decoded query parameters of a URL or a relative reference, in order.
/// `+` decodes to a space, as in a form; invalid `%` escapes are kept as is.
pub fn query_params(url: &str) -> Vec<(String, String)> {
    let without_fragment = url.split('#').next().unwrap_or("");
    let Some((_, query)) = without_fragment.split_once('?') else {
        return Vec::new();
    };
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (k, v) = part.split_once('=').unwrap_or((part, ""));
            (decode_component(k), decode_component(v))
        })
        .collect()
}

fn decode_component(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => match (bytes.get(i + 1), bytes.get(i + 2)) {
                (Some(&h), Some(&l)) if h.is_ascii_hexdigit() && l.is_ascii_hexdigit() => {
                    out.push(hex_value(h) << 4 | hex_value(l));
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(digit: u8) -> u8 {
    match digit {
        b'0'..=b'9' => digit - b'0',
        b'a'..=b'f' => digit - b'a' + 10,
        _ => digit - b'A' + 10,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_request() {
        let r = Request::new(Method::Post, "https://example.test/x")
            .header("Content-Type", "application/json")
            .body("{}");
        assert_eq!(r.method.as_str(), "POST");
        assert_eq!(r.header_value("content-type"), Some("application/json"));
        assert_eq!(r.body.as_deref(), Some("{}"));
        assert!(r.follow_redirects);
        assert!(!r.no_redirects().follow_redirects);
    }

    #[test]
    fn success_range() {
        assert!(Response::new(204, "").is_success());
        assert!(!Response::new(401, "").is_success());
    }

    #[test]
    fn response_headers_are_case_insensitive() {
        let mut r = Response::new(302, "");
        r.headers
            .push(("location".into(), "https://example.test/?code=1".into()));
        assert_eq!(
            r.header_value("Location"),
            Some("https://example.test/?code=1")
        );
        assert_eq!(r.header_value("Set-Cookie"), None);
    }

    #[test]
    fn encodes_like_quote_plus() {
        assert_eq!(encode_component("aZ09-_.~"), "aZ09-_.~");
        assert_eq!(encode_component("a b+c/d@e:f"), "a+b%2Bc%2Fd%40e%3Af");
        assert_eq!(encode_component("é"), "%C3%A9");
        assert_eq!(encode_component(""), "");
    }

    #[test]
    fn form_keeps_order_and_empty_values() {
        assert_eq!(
            form_encode(&[("b", "2"), ("a", ""), ("redirect_uri", "https://x.test/r")]),
            "b=2&a=&redirect_uri=https%3A%2F%2Fx.test%2Fr"
        );
        assert_eq!(form_encode(&[]), "");
    }

    #[test]
    fn decodes_query_params() {
        let q = query_params(
            "https://x.test/cb?code=C%2B1&error_description=Invalid+credentials&flag#frag",
        );
        assert_eq!(
            q,
            vec![
                ("code".to_string(), "C+1".to_string()),
                (
                    "error_description".to_string(),
                    "Invalid credentials".to_string()
                ),
                ("flag".to_string(), String::new()),
            ]
        );
        assert_eq!(
            query_params("/relative?a=%C3%A9&bad=%zz"),
            vec![
                ("a".to_string(), "é".to_string()),
                ("bad".to_string(), "%zz".to_string()),
            ]
        );
        assert!(query_params("https://x.test/no-query").is_empty());
    }
}
