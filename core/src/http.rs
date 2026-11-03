//! Plain HTTP values exchanged with the Swift shell.
//!
//! The core produces [`Request`]s and consumes [`Response`]s. The shell sends
//! them with URLSession and hands the bytes back unchanged.

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
}

impl Request {
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Value of the first header with this name, compared case-insensitively.
    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Response {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
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
    }

    #[test]
    fn success_range() {
        assert!(Response {
            status: 204,
            body: String::new()
        }
        .is_success());
        assert!(!Response {
            status: 401,
            body: String::new()
        }
        .is_success());
    }
}
