//! Bluelink EU protocol (Hyundai OneApp / CCI login, legacy SPA vehicle API).
//!
//! Reference: Hyundai-Kia-Connect/hyundai_kia_connect_api (MIT), region 1
//! (`KiaUvoApiEU`). See docs/STATE.md for the request chain. The order of a
//! full run, each step a [`Request`](crate::http::Request) the shell sends:
//!
//! 1. [`spa::register_request`]: device id (`Stamp`-signed).
//! 2. [`idp::authorize_request`], [`idp::certs_request`], RSA in Swift,
//!    [`idp::signin_request`]: auth code. One cookie store for the three.
//! 3. [`cci::token_request`], then [`cci::ccs_exchange_request`]: CCS token
//!    (`X-Fingerprint`-signed, valid about 1 h).
//! 4. [`spa::vehicles_request`], [`spa::status_request`]: vehicle and charge.
//! 5. [`spa::pin_request`]: control token from the Bluelink PIN, then
//!    [`spa::send_destination_request`].
//!
//! The core takes time, random values and secrets as inputs; it never
//! generates or stores them.

pub mod cci;
pub mod fingerprint;
pub mod idp;
pub mod spa;
pub mod stamp;

#[cfg(test)]
mod fixtures;

use fingerprint::SigningKey;

/// App-embedded values of the official OneApp. They can change with app
/// releases, so they are data, loaded with [`AppConstants::from_json`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct AppConstants {
    /// e.g. `https://idpconnect-eu.hyundai.com`
    pub idp_host: String,
    /// e.g. `https://cci-api-eu.hyundai.com`
    pub cci_host: String,
    /// e.g. `https://prd.eu-ccapi.hyundai.com:8080`
    pub spa_host: String,
    pub oneapp_client_id: String,
    pub oneapp_redirect_uri: String,
    pub ccsp_service_id: String,
    pub app_id: String,
    pub cfb_base64: String,
    pub push_type: String,
    pub idp_user_agent: String,
    pub spa_user_agent: String,
    pub client: ClientIdentity,
    pub signing_key: SigningKey,
    /// `cfb_base64` decoded, filled by [`AppConstants::from_json`].
    #[serde(skip)]
    pub cfb: Vec<u8>,
}

/// The OneApp build identity the CCI fingerprint gate checks.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct ClientIdentity {
    pub package_id: String,
    pub client_name: String,
    pub version: String,
    pub os_code: String,
    pub os_version: String,
    pub device_model: String,
    pub notification_provider: String,
    pub user_agent: String,
    pub brand_label: String,
    pub region: String,
}

impl AppConstants {
    /// Parses and checks the constants; unknown fields are ignored.
    pub fn from_json(text: &str) -> Result<Self, Error> {
        let invalid = Error::Parse("constants");
        let mut constants: Self = serde_json::from_str(text).map_err(|_| invalid.clone())?;
        constants.cfb = crate::base64::decode(&constants.cfb_base64).ok_or(invalid.clone())?;
        if constants.cfb.is_empty() {
            return Err(invalid);
        }
        Ok(constants)
    }
}

/// Language and UTC offset of the phone, sent on the CCI calls.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Locale {
    /// Two-letter code, e.g. `fr`.
    pub language: String,
    /// e.g. `+01:00`.
    pub timezone: String,
}

/// A bearer token and its lifetime as the server announced it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Token {
    /// Raw token; request builders add the `Bearer ` prefix.
    pub access_token: String,
    pub expires_in_s: u32,
}

/// Why a step failed. [`Error::code`] is the short reason the Diagnostic
/// line shows: never a token, a name or a message from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Unexpected HTTP status.
    Http(u16),
    /// The IDP web firewall blocked the login ("abusing request").
    Waf,
    InvalidCredentials,
    /// Sign-in rejected for another reason (e.g. a locked account).
    SigninRejected,
    /// The account must accept new terms in a browser once.
    ConsentRequired,
    /// Sign-in sent back to the login page. Some EU accounts only accept
    /// the first 20 characters of a longer password.
    BackToLogin {
        long_password: bool,
    },
    UnexpectedRedirect,
    /// Fingerprint gate: 9035 missing, 9036 bad signature, 9037 timestamp
    /// out of window. 9035 and 9036 mean the app constants need renewing.
    Gate(u16),
    /// SPA API failure code (`resCode`), e.g. 4002 invalid device id.
    Api(String),
    /// The CCS token is no longer accepted: log in again.
    TokenExpired,
    /// No control token: wrong Bluelink PIN.
    Pin,
    /// A field the step needs is missing or malformed.
    Parse(&'static str),
}

impl Error {
    pub fn code(&self) -> String {
        match self {
            Error::Http(status) => format!("http{status}"),
            Error::Waf => "waf".into(),
            Error::InvalidCredentials => "creds".into(),
            Error::SigninRejected => "signin".into(),
            Error::ConsentRequired => "consent".into(),
            Error::BackToLogin {
                long_password: false,
            } => "login".into(),
            Error::BackToLogin {
                long_password: true,
            } => "login20".into(),
            Error::UnexpectedRedirect => "redirect".into(),
            Error::Gate(code) => format!("gate{code}"),
            Error::Api(code) => {
                let lead: String = code
                    .trim()
                    .chars()
                    .take_while(char::is_ascii_alphanumeric)
                    .take(8)
                    .collect();
                if lead.is_empty() {
                    "api".into()
                } else {
                    lead
                }
            }
            Error::TokenExpired => "token".into(),
            Error::Pin => "pin".into(),
            Error::Parse(what) => format!("parse-{what}"),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.code())
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_constants_and_decodes_cfb() {
        let c = fixtures::constants();
        assert_eq!(c.client.client_name, "ExampleEU");
        assert_eq!(c.cfb.len(), 50);
        assert_eq!(c.cfb[0], 11);
    }

    #[test]
    fn rejects_constants_with_bad_cfb_or_missing_fields() {
        let raw = fixtures::text("constants_synthetic.json");
        let bad_cfb = raw.replace(&fixtures::constants().cfb_base64, "not base64!");
        assert_eq!(
            AppConstants::from_json(&bad_cfb).unwrap_err().code(),
            "parse-constants"
        );
        assert_eq!(
            AppConstants::from_json("{}").unwrap_err().code(),
            "parse-constants"
        );
    }

    #[test]
    fn codes_fit_the_diagnostic_line() {
        let all = [
            Error::Http(503),
            Error::Waf,
            Error::InvalidCredentials,
            Error::SigninRejected,
            Error::ConsentRequired,
            Error::BackToLogin {
                long_password: true,
            },
            Error::UnexpectedRedirect,
            Error::Gate(9036),
            Error::Api("4002 invalid device".into()),
            Error::TokenExpired,
            Error::Pin,
            Error::Parse("certs"),
        ];
        for e in all {
            let code = e.code();
            assert!(!code.is_empty() && code.len() <= 16, "{code}");
            assert!(
                code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                "{code}"
            );
        }
        assert_eq!(Error::Api("4002 invalid device".into()).code(), "4002");
        assert_eq!(Error::Api(" ?".into()).code(), "api");
    }
}
