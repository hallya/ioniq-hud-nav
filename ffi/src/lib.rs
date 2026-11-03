//! UniFFI bridge: the only place that knows about Swift.
//! Keep it thin; logic lives in `hudnav-core`.

use hudnav_core::{diag, geo};

uniffi::setup_scaffolding!();

#[derive(uniffi::Enum)]
pub enum CheckState {
    Pending,
    Ok,
    Ko { reason: String },
}

impl From<CheckState> for diag::Check {
    fn from(c: CheckState) -> Self {
        match c {
            CheckState::Pending => diag::Check::Pending,
            CheckState::Ok => diag::Check::Ok,
            CheckState::Ko { reason } => diag::Check::Ko(reason),
        }
    }
}

#[uniffi::export]
pub fn core_version() -> String {
    hudnav_core::VERSION.to_string()
}

#[uniffi::export]
pub fn diagnostic_line(
    auth: CheckState,
    vehicle: CheckState,
    soc: Option<u8>,
    send: CheckState,
    plan_ms: Option<u32>,
) -> String {
    diag::Report {
        auth: auth.into(),
        vehicle: vehicle.into(),
        soc,
        send: send.into(),
        plan_ms,
    }
    .line()
}

#[uniffi::export]
pub fn distance_m(a_lat: f64, a_lon: f64, b_lat: f64, b_lon: f64) -> f64 {
    geo::LatLon::new(a_lat, a_lon).distance_m(&geo::LatLon::new(b_lat, b_lon))
}

// ---- Bluelink run -------------------------------------------------------

use hudnav_core::bluelink::{self, run, spa, upstream::FingerprintOverride};
use hudnav_core::http;
use std::sync::{Arc, Mutex};

/// Random bytes a run needs; Swift fills them with `SecRandomCopyBytes`.
#[uniffi::export]
pub fn bluelink_entropy_len() -> u32 {
    run::ENTROPY_LEN as u32
}

#[derive(uniffi::Record)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(uniffi::Record)]
pub struct HttpRequest {
    /// `GET`, `POST` or `PUT`.
    pub method: String,
    pub url: String,
    /// In order; send them as given.
    pub headers: Vec<Header>,
    pub body: Option<String>,
    /// False: return a 3xx as is, with its `Location`.
    pub follow_redirects: bool,
}

#[derive(uniffi::Record)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<Header>,
    /// Final URL after redirects.
    pub url: String,
    pub body: String,
}

#[derive(uniffi::Record)]
pub struct Destination {
    pub lat: f64,
    pub lon: f64,
    pub name: String,
    pub addr: String,
    pub zip: String,
}

#[derive(uniffi::Enum)]
pub enum RunStep {
    Send {
        request: HttpRequest,
    },
    /// RSA PKCS#1 v1.5 encryption of the password (UTF-8) under this
    /// PKCS#1 `RSAPublicKey` DER; answer with `RunInput::Encrypted`.
    EncryptPassword {
        public_key_der: Vec<u8>,
    },
    Done {
        line: String,
        all_ok: bool,
        repair_signature: bool,
    },
}

#[derive(uniffi::Enum)]
pub enum RunInput {
    Received {
        response: HttpResponse,
    },
    /// Empty when the encryption failed.
    Encrypted {
        ciphertext: Vec<u8>,
    },
    /// The request failed: the `URLError` code (-1009 offline, -1200 TLS…),
    /// 0 when unknown.
    Failed {
        code: i32,
    },
}

#[derive(Debug, uniffi::Error)]
pub enum BridgeError {
    /// A short code, safe for the Diagnostic line.
    Invalid { code: String },
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::Invalid { code } => f.write_str(code),
        }
    }
}

impl std::error::Error for BridgeError {}

/// One Bluelink chain: login, vehicle, charge, then the destination.
#[derive(uniffi::Object)]
pub struct BluelinkRun {
    inner: Mutex<run::Run>,
}

#[uniffi::export]
impl BluelinkRun {
    /// `destination` empty: a connection check that sends nothing.
    #[allow(clippy::too_many_arguments)]
    #[uniffi::constructor]
    pub fn new(
        constants_json: String,
        override_json: Option<String>,
        username: String,
        password_chars: u32,
        pin: String,
        language: String,
        timezone: String,
        entropy: Vec<u8>,
        destination: Vec<Destination>,
    ) -> Arc<Self> {
        let inputs = run::Inputs {
            username,
            password_chars: password_chars as usize,
            pin,
            locale: bluelink::Locale { language, timezone },
            entropy,
            destination: destination
                .into_iter()
                .map(|d| spa::Poi {
                    lat: d.lat,
                    lon: d.lon,
                    name: d.name,
                    addr: d.addr,
                    zip: d.zip,
                })
                .collect(),
        };
        Arc::new(Self {
            inner: Mutex::new(run::Run::new(
                &constants_json,
                override_json.as_deref(),
                inputs,
            )),
        })
    }

    pub fn start(&self, now_ms: u64) -> RunStep {
        step(self.lock().start(now_ms))
    }

    pub fn resume(&self, input: RunInput, now_ms: u64) -> RunStep {
        let input = match input {
            RunInput::Received { response } => run::Input::Received(http::Response {
                status: response.status,
                headers: response
                    .headers
                    .into_iter()
                    .map(|h| (h.name, h.value))
                    .collect(),
                url: response.url,
                body: response.body,
            }),
            RunInput::Encrypted { ciphertext } => run::Input::Encrypted(ciphertext),
            RunInput::Failed { code } => run::Input::Failed(code),
        };
        step(self.lock().resume(input, now_ms))
    }
}

impl BluelinkRun {
    fn lock(&self) -> std::sync::MutexGuard<'_, run::Run> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn step(effect: run::Effect) -> RunStep {
    match effect {
        run::Effect::Send(r) => RunStep::Send {
            request: HttpRequest {
                method: r.method.as_str().to_string(),
                url: r.url,
                headers: r
                    .headers
                    .into_iter()
                    .map(|(name, value)| Header { name, value })
                    .collect(),
                body: r.body,
                follow_redirects: r.follow_redirects,
            },
        },
        run::Effect::EncryptPassword { public_key_der } => {
            RunStep::EncryptPassword { public_key_der }
        }
        run::Effect::Done(o) => RunStep::Done {
            line: o.report.line(),
            all_ok: o.report.all_ok(),
            repair_signature: o.repair_signature,
        },
    }
}

/// Reads the signature repair from the text of upstream's
/// `cci_fingerprint.py`; returns the override as JSON to store.
#[uniffi::export]
pub fn fingerprint_override_from_upstream(source: String) -> Result<String, BridgeError> {
    let o = FingerprintOverride::from_upstream(&source)
        .map_err(|e| BridgeError::Invalid { code: e.code() })?;
    serde_json::to_string(&o).map_err(|_| BridgeError::Invalid {
        code: "json".into(),
    })
}

/// A public landmark for the step 1 test: never a user's own place.
#[uniffi::export]
pub fn test_destination() -> Destination {
    Destination {
        lat: 48.858_26,
        lon: 2.294_5,
        name: "Tour Eiffel".into(),
        addr: "Champ de Mars, 5 Avenue Anatole France, Paris".into(),
        zip: "75007".into(),
    }
}
