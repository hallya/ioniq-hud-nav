//! CCI token exchanges, step 4-5: auth code → CCI tokens → CCS token.
//!
//! Both calls are fingerprint-gated since 2026-10-14 (upstream `cci_auth.py`):
//! they carry the OneApp identity headers, `X-Timestamp` and `X-Fingerprint`.
//! Upstream's unsigned legacy fallback is not ported: it is marked
//! transitional there, and a gate rejection must surface as [`Error::Gate`]
//! so the app can renew its constants.

use super::fingerprint::x_fingerprint;
use super::{AppConstants, Error, Locale, Token};
use crate::http::{form_encode, Method, Request, Response};
use serde_json::Value;

/// Per-call values the shell supplies: no clock or randomness in the core.
#[derive(Debug, Clone, Copy)]
pub struct Signing<'a> {
    /// From [`super::spa::parse_device_id`].
    pub device_id: &'a str,
    /// Current time in milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
    /// 24 random upper-case hex characters, new for each request.
    pub request_nonce: &'a str,
    pub locale: &'a Locale,
}

/// The CCI token set. Only `access_token` is required; the others are kept
/// for the CCS exchange and a later token refresh.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CciTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub non_ccs_token: String,
    pub exchangeable_access_token: String,
    pub exchangeable_refresh_token: String,
    pub non_ccs_refresh_token: String,
    pub id_token: String,
    pub expires_in_s: u32,
}

pub fn token_request(c: &AppConstants, s: &Signing, code: &str) -> Request {
    let url = format!(
        "{}/auth/v1/token?{}",
        c.cci_host,
        form_encode(&[("code", code)])
    );
    Request::new(Method::Post, url).headers(signed_headers(c, s, ""))
}

pub fn parse_tokens(r: &Response) -> Result<CciTokens, Error> {
    let body = checked(r)?;
    let text = |key: &str| body[key].as_str().unwrap_or("").to_string();
    let tokens = CciTokens {
        access_token: text("accessToken"),
        refresh_token: text("refreshToken"),
        non_ccs_token: text("nonCcsToken"),
        exchangeable_access_token: text("exchangeableAccessToken"),
        exchangeable_refresh_token: text("exchangeableRefreshToken"),
        non_ccs_refresh_token: text("nonCcsRefreshToken"),
        id_token: text("idToken"),
        expires_in_s: seconds(&body["expiresIn"]).unwrap_or(3599),
    };
    if tokens.access_token.is_empty() {
        return Err(Error::Parse("cci"));
    }
    Ok(tokens)
}

pub fn ccs_exchange_request(c: &AppConstants, s: &Signing, t: &CciTokens) -> Request {
    let access = t.access_token.trim_start_matches("Bearer ").trim();
    let mut headers = signed_headers(c, s, "");
    headers.push(("authorization".into(), format!("Bearer {access}")));
    headers.push(("Authentication".into(), t.non_ccs_token.clone()));
    if !t.exchangeable_access_token.is_empty() {
        headers.push((
            "exchangeable-token".into(),
            t.exchangeable_access_token.clone(),
        ));
        headers.push(("non-ccs-token".into(), t.non_ccs_token.clone()));
    }
    let url = format!("{}/auth/v1/token-exchange?serviceType=CCS", c.cci_host);
    Request::new(Method::Post, url).headers(headers)
}

/// The CCS token, accepted as a bearer token by the SPA API.
pub fn parse_ccs(r: &Response) -> Result<Token, Error> {
    let body = checked(r)?;
    let token = ["accessToken", "ccsAccessToken"]
        .iter()
        .filter_map(|key| body[*key].as_str())
        .find(|t| !t.is_empty())
        .ok_or(Error::Parse("ccs"))?;
    Ok(Token {
        access_token: token.to_string(),
        expires_in_s: seconds(&body["expiresTime"]).unwrap_or(3600),
    })
}

/// Upstream `build_fp_headers` + `X-Timestamp` + `X-Fingerprint`, in order.
fn signed_headers(c: &AppConstants, s: &Signing, body: &str) -> Vec<(String, String)> {
    let id = &c.client;
    let device_compact: String = s.device_id.chars().filter(|ch| *ch != '-').collect();
    let mut headers: Vec<(String, String)> = [
        ("client-id", id.package_id.clone()),
        ("client-name", id.client_name.clone()),
        ("client-version", id.version.clone()),
        ("client-os-code", id.os_code.clone()),
        ("client-os-version", id.os_version.clone()),
        ("client-device-id", s.device_id.to_string()),
        ("client-device-model", id.device_model.clone()),
        (
            "client-notification-provider-type",
            id.notification_provider.clone(),
        ),
        ("locale", s.locale.language.to_uppercase()),
        ("timezone", s.locale.timezone.clone()),
        (
            "App-Request-Id",
            format!("{}_{device_compact}_AOS", s.request_nonce),
        ),
        ("X-Brand", id.brand_label.clone()),
        ("X-Region", id.region.clone()),
        ("User-Agent", id.user_agent.clone()),
        ("Accept", "*/*".to_string()),
        ("Accept-Language", s.locale.language.to_lowercase()),
        ("X-Timestamp", s.timestamp_ms.to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let signature = x_fingerprint(&headers, body, &c.signing_key);
    headers.push(("X-Fingerprint".into(), signature));
    headers
}

/// The JSON body of a 200, or the error the status and body point to.
fn checked(r: &Response) -> Result<Value, Error> {
    let body: Option<Value> = serde_json::from_str(&r.body).ok();
    if r.status == 200 {
        return body.ok_or(Error::Parse("cci"));
    }
    let gate = body
        .as_ref()
        .and_then(|b| b["code"].as_u64())
        .filter(|code| (9035..=9037).contains(code));
    match gate {
        Some(code) if (400..500).contains(&r.status) => Err(Error::Gate(code as u16)),
        _ => Err(Error::Http(r.status)),
    }
}

/// A lifetime in seconds, sent as a number or a numeric string.
fn seconds(v: &Value) -> Option<u32> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
}

#[cfg(test)]
mod tests {
    use super::super::fixtures;
    use super::*;

    fn with_signing(f: &serde_json::Value, run: impl FnOnce(&Signing)) {
        let i = &f["inputs"];
        let locale = fixtures::locale(&i["locale"]);
        let s = Signing {
            device_id: fixtures::str(i, "device_id"),
            timestamp_ms: i["timestamp_ms"].as_u64().expect("timestamp_ms"),
            request_nonce: fixtures::str(i, "request_nonce"),
            locale: &locale,
        };
        run(&s);
    }

    #[test]
    fn token() {
        let f = fixtures::json("cci.json");
        let step = &f["token"];
        with_signing(&f, |s| {
            let built = token_request(&fixtures::constants(), s, fixtures::str(step, "code"));
            fixtures::assert_request(&built, &step["request"]);
        });
        fixtures::check_cases(step, |_, r| parse_tokens(r));
    }

    #[test]
    fn ccs_exchange() {
        let f = fixtures::json("cci.json");
        let step = &f["ccs_exchange"];
        let tokens: CciTokens = serde_json::from_value(step["tokens"].clone()).expect("tokens");
        with_signing(&f, |s| {
            let c = fixtures::constants();
            fixtures::assert_request(&ccs_exchange_request(&c, s, &tokens), &step["request"]);
            let bare = CciTokens {
                exchangeable_access_token: String::new(),
                ..tokens.clone()
            };
            fixtures::assert_request(
                &ccs_exchange_request(&c, s, &bare),
                &step["request_without_exchangeable"],
            );
        });
        fixtures::check_cases(step, |_, r| parse_ccs(r));
    }

    #[test]
    fn the_signature_covers_the_timestamp() {
        let f = fixtures::json("cci.json");
        with_signing(&f, |s| {
            let c = fixtures::constants();
            let later = Signing {
                timestamp_ms: s.timestamp_ms + 1,
                ..*s
            };
            let a = token_request(&c, s, "x");
            let b = token_request(&c, &later, "x");
            assert_ne!(
                a.header_value("X-Fingerprint"),
                b.header_value("X-Fingerprint")
            );
        });
    }
}
