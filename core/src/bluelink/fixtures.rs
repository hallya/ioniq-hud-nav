//! Test helpers: load `fixtures/bluelink_eu/*.json` (written by
//! `scripts/gen-bluelink-fixtures.py`) and compare the core against them.

use super::{AppConstants, Error, Locale};
use crate::http::{Method, Request, Response};
use serde_json::Value;

pub fn text(name: &str) -> String {
    let path = format!(
        "{}/../fixtures/bluelink_eu/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

pub fn json(name: &str) -> Value {
    serde_json::from_str(&text(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

pub fn constants() -> AppConstants {
    AppConstants::from_json(&text("constants_synthetic.json")).expect("synthetic constants load")
}

pub fn locale(v: &Value) -> Locale {
    serde_json::from_value(v.clone()).expect("locale")
}

pub fn str<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key]
        .as_str()
        .unwrap_or_else(|| panic!("fixture field {key} is not a string"))
}

pub fn request(v: &Value) -> Request {
    let method = match str(v, "method") {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        other => panic!("method {other}"),
    };
    Request {
        method,
        url: str(v, "url").to_string(),
        headers: pairs(&v["headers"]),
        body: v["body"].as_str().map(str::to_string),
        follow_redirects: v["follow_redirects"].as_bool().expect("follow_redirects"),
    }
}

pub fn response(v: &Value) -> Response {
    Response {
        status: v["status"].as_u64().expect("status") as u16,
        headers: pairs(&v["headers"]),
        url: str(v, "url").to_string(),
        body: str(v, "body").to_string(),
    }
}

fn pairs(v: &Value) -> Vec<(String, String)> {
    v.as_array()
        .expect("headers array")
        .iter()
        .map(|p| {
            (
                p[0].as_str().expect("name").to_string(),
                p[1].as_str().expect("value").to_string(),
            )
        })
        .collect()
}

/// Asserts a built request equals the fixture, header by header first so a
/// failure names the header that differs.
pub fn assert_request(got: &Request, expected: &Value) {
    let want = request(expected);
    assert_eq!(got.method, want.method, "method");
    assert_eq!(got.url, want.url, "url");
    for (i, (g, w)) in got.headers.iter().zip(&want.headers).enumerate() {
        assert_eq!(g, w, "header #{i}");
    }
    assert_eq!(got.headers.len(), want.headers.len(), "header count");
    assert_eq!(got.body, want.body, "body");
    assert_eq!(
        got.follow_redirects, want.follow_redirects,
        "follow_redirects"
    );
}

/// Runs a parser over every `responses` case of a fixture step. A case
/// expects either `{"ok": <value as JSON>}` or `{"err": "<Error::code>"}`.
pub fn check_cases<T: serde::Serialize>(
    step: &Value,
    mut parse: impl FnMut(&Value, &Response) -> Result<T, Error>,
) {
    let cases = step["responses"].as_array().expect("responses");
    assert!(!cases.is_empty());
    for case in cases {
        let name = str(case, "name");
        let got = parse(case, &response(&case["response"]));
        let expect = &case["expect"];
        match (got, expect.get("ok"), expect.get("err")) {
            (Ok(value), Some(ok), None) => assert_eq!(
                &serde_json::to_value(value).expect("serializes"),
                ok,
                "case: {name}"
            ),
            (Err(e), None, Some(err)) => {
                assert_eq!(Some(e.code().as_str()), err.as_str(), "case: {name}")
            }
            (Ok(value), _, _) => panic!(
                "case {name}: expected {expect}, got Ok({})",
                serde_json::to_value(value).expect("serializes")
            ),
            (Err(e), _, _) => panic!("case {name}: expected {expect}, got Err({})", e.code()),
        }
    }
}
