//! IDP login, steps 1-3: authorize page, RSA public key, sign-in.
//!
//! Upstream `KiaUvoApiEU._login_with_password_cci`. The three requests share
//! one cookie store. The password is RSA-PKCS1 encrypted in Swift (Security
//! framework) with the key from [`parse_certs`]; only its hex reaches the core.

use super::{AppConstants, Error};
use crate::http::{form_encode, query_params, Method, Request, Response};
use serde_json::Value;

/// The JWK the password is encrypted with. `n` and `e` are base64url.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RsaKey {
    pub kid: String,
    pub n: String,
    pub e: String,
}

/// Upstream sends the redirect URI unencoded and fixes `lang` and `country`;
/// kept as is, since that is the request known to pass the firewall.
pub fn authorize_request(c: &AppConstants) -> Request {
    let url = format!(
        "{}/auth/api/v2/user/oauth2/authorize?response_type=code&client_id={}\
         &redirect_uri={}&lang=en&state=ccsp&country=de",
        c.idp_host, c.oneapp_client_id, c.oneapp_redirect_uri
    );
    Request::new(Method::Get, url).header("User-Agent", &c.idp_user_agent)
}

/// Detects the web firewall block; any other page is fine.
pub fn check_authorize(r: &Response) -> Result<(), Error> {
    if r.body.to_ascii_lowercase().contains("abusing") || r.url.contains("/error?status=400") {
        return Err(Error::Waf);
    }
    if r.status >= 400 {
        return Err(Error::Http(r.status));
    }
    Ok(())
}

pub fn certs_request(c: &AppConstants) -> Request {
    Request::new(
        Method::Get,
        format!("{}/auth/api/v1/accounts/certs", c.idp_host),
    )
    .header("User-Agent", &c.idp_user_agent)
}

pub fn parse_certs(r: &Response) -> Result<RsaKey, Error> {
    if r.status != 200 {
        return Err(Error::Http(r.status));
    }
    let invalid = Error::Parse("certs");
    let body: Value = serde_json::from_str(&r.body).map_err(|_| invalid.clone())?;
    let jwk = &body["retValue"];
    let field = |name: &str| {
        jwk[name]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    match (field("n"), field("e")) {
        (Some(n), Some(e)) => Ok(RsaKey {
            kid: field("kid").unwrap_or_default(),
            n,
            e,
        }),
        _ => Err(invalid),
    }
}

pub fn signin_request(
    c: &AppConstants,
    username: &str,
    encrypted_password_hex: &str,
    kid: &str,
) -> Request {
    let form = form_encode(&[
        ("client_id", &c.oneapp_client_id),
        ("encryptedPassword", "true"),
        ("password", encrypted_password_hex),
        ("redirect_uri", &c.oneapp_redirect_uri),
        ("scope", ""),
        ("nonce", ""),
        ("state", "ccsp"),
        ("username", username),
        ("connector_session_key", ""),
        ("kid", kid),
        ("_csrf", ""),
    ]);
    Request::new(Method::Post, format!("{}/auth/account/signin", c.idp_host))
        .header("User-Agent", &c.idp_user_agent)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form)
        .no_redirects()
}

/// The auth code from the sign-in redirect. `password_chars` only selects
/// the hint of [`Error::BackToLogin`].
pub fn parse_signin(r: &Response, password_chars: usize) -> Result<String, Error> {
    if r.status != 302 {
        return Err(Error::Http(r.status));
    }
    let location = r.header_value("Location").unwrap_or("");
    let query = query_params(location);
    let param = |name: &str| {
        query
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    if let Some(code) = param("code").filter(|c| !c.is_empty()) {
        return Ok(code.to_string());
    }
    if param("error").is_some() {
        let description = param("error_description").unwrap_or("").trim();
        return Err(if description.eq_ignore_ascii_case("invalid credentials") {
            Error::InvalidCredentials
        } else {
            Error::SigninRejected
        });
    }
    if location.contains("/web/v1/user/authorization") {
        return Err(Error::ConsentRequired);
    }
    if location.contains("authorize") {
        return Err(Error::BackToLogin {
            long_password: password_chars > 20,
        });
    }
    Err(Error::UnexpectedRedirect)
}

#[cfg(test)]
mod tests {
    use super::super::fixtures;
    use super::*;

    #[test]
    fn authorize() {
        let f = fixtures::json("idp.json");
        let step = &f["authorize"];
        fixtures::assert_request(&authorize_request(&fixtures::constants()), &step["request"]);
        fixtures::check_cases(step, |_, r| check_authorize(r));
    }

    #[test]
    fn certs() {
        let f = fixtures::json("idp.json");
        let step = &f["certs"];
        fixtures::assert_request(&certs_request(&fixtures::constants()), &step["request"]);
        fixtures::check_cases(step, |_, r| parse_certs(r));
    }

    #[test]
    fn signin() {
        let f = fixtures::json("idp.json");
        let step = &f["signin"];
        let i = &step["inputs"];
        let built = signin_request(
            &fixtures::constants(),
            fixtures::str(i, "username"),
            fixtures::str(i, "encrypted_password_hex"),
            fixtures::str(i, "kid"),
        );
        fixtures::assert_request(&built, &step["request"]);
        fixtures::check_cases(step, |case, r| {
            parse_signin(
                r,
                case["password_chars"].as_u64().expect("password_chars") as usize,
            )
        });
    }
}
