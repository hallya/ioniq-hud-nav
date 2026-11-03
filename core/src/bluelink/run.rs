//! The whole chain as one sans-IO run: device id, login, CCS token, vehicle,
//! charge, PIN, send destination. It ends with the Diagnostic report.
//!
//! The shell loops: it calls [`Run::start`], performs each [`Effect`] and
//! hands the result back with [`Run::resume`], until [`Effect::Done`]. It
//! passes the clock on every call; randomness comes once, as
//! [`Inputs::entropy`]. The password never enters the core: the run asks the
//! shell to encrypt it ([`Effect::EncryptPassword`]) and gets the ciphertext.
//!
//! One run is one login: the shell uses a fresh cookie store for it.

use super::{cci, idp, spa, upstream::FingerprintOverride, AppConstants, Error, Locale, Token};
use crate::diag::{Check, Report};
use crate::http::{Request, Response};

/// Bytes of [`Inputs::entropy`]: a UUID, a push id and two request nonces.
pub const ENTROPY_LEN: usize = UUID_LEN + PUSH_LEN + 2 * NONCE_LEN;
const UUID_LEN: usize = 16;
const PUSH_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// What the user and the phone give a run.
#[derive(Debug, Clone, PartialEq)]
pub struct Inputs {
    pub username: String,
    /// Characters in the password; only picks the `login20` hint.
    pub password_chars: usize,
    pub pin: String,
    pub locale: Locale,
    /// [`ENTROPY_LEN`] random bytes from the shell.
    pub entropy: Vec<u8>,
    /// Waypoints, the last one the destination. Empty: stop after the
    /// charge, a connection check that sends nothing to the car.
    pub destination: Vec<spa::Poi>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Send this request, answer with [`Input::Received`] or
    /// [`Input::Failed`].
    Send(Request),
    /// Encrypt the password (UTF-8) with RSA PKCS#1 v1.5 under this
    /// PKCS#1 `RSAPublicKey`, answer with [`Input::Encrypted`].
    EncryptPassword {
        public_key_der: Vec<u8>,
    },
    Done(Outcome),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Received(Response),
    /// The ciphertext; empty when the encryption failed.
    Encrypted(Vec<u8>),
    /// The request could not be sent or answered: the shell's error code
    /// (`URLError` code, e.g. -1009 offline, -1200 TLS), 0 when unknown.
    Failed(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub report: Report,
    /// The fingerprint gate rejected the app constants (`gate9035` or
    /// `gate9036`): offer "Update signature", then a new run.
    pub repair_signature: bool,
}

pub struct Run {
    setup: Result<Setup, Error>,
    stage: Stage,
    report: Report,
    repair_signature: bool,
}

/// Everything fixed when the run starts.
struct Setup {
    c: AppConstants,
    inputs: Inputs,
    uuid: String,
    push_reg_id: String,
    nonces: [String; 2],
}

/// What the run waits for, with what it has learned so far.
enum Stage {
    NotStarted,
    Register,
    Authorize { device_id: String },
    Certs { device_id: String },
    Encrypt { device_id: String, kid: String },
    Signin { device_id: String },
    Token { device_id: String },
    Ccs { device_id: String },
    Vehicles { s: Auth },
    Status { s: Auth, v: spa::Vehicle },
    Pin { s: Auth, v: spa::Vehicle },
    Send,
    Done,
}

/// The CCS session the SPA calls share.
struct Auth {
    device_id: String,
    ccs: Token,
}

/// The Diagnostic check a failure at a stage lands in.
#[derive(Clone, Copy)]
enum Phase {
    Auth,
    Vehicle,
    Send,
}

impl Stage {
    fn phase(&self) -> Phase {
        match self {
            Stage::Vehicles { .. } => Phase::Vehicle,
            Stage::Status { .. } | Stage::Pin { .. } | Stage::Send => Phase::Send,
            _ => Phase::Auth,
        }
    }
}

impl Run {
    /// `override_json`: a stored [`FingerprintOverride`] from a signature
    /// repair, applied over the bundled constants. Setup errors surface on
    /// [`Run::start`] as `auth ko:parse-…`.
    pub fn new(constants_json: &str, override_json: Option<&str>, inputs: Inputs) -> Run {
        Run {
            setup: Setup::new(constants_json, override_json, inputs),
            stage: Stage::NotStarted,
            report: Report::default(),
            repair_signature: false,
        }
    }

    pub fn start(&mut self, now_ms: u64) -> Effect {
        if !matches!(self.stage, Stage::NotStarted) {
            return self.finished();
        }
        let setup = match &self.setup {
            Ok(setup) => setup,
            Err(e) => {
                let e = e.clone();
                return self.fail(Phase::Auth, &e);
            }
        };
        let request =
            spa::register_request(&setup.c, now_ms / 1000, &setup.push_reg_id, &setup.uuid);
        self.stage = Stage::Register;
        Effect::Send(request)
    }

    pub fn resume(&mut self, input: Input, now_ms: u64) -> Effect {
        let stage = std::mem::replace(&mut self.stage, Stage::Done);
        let phase = stage.phase();
        match self.step(stage, input, now_ms) {
            Ok(effect) => effect,
            Err(e) => self.fail(phase, &e),
        }
    }

    fn step(&mut self, stage: Stage, input: Input, now_ms: u64) -> Result<Effect, Error> {
        let Ok(setup) = &self.setup else {
            return Err(Error::Parse("seq"));
        };
        let c = &setup.c;
        let signing = |device_id, nonce| setup.signing(device_id, nonce, now_ms);
        let session = |a| Auth::session(a, now_ms);
        let (next, effect) = match (stage, input) {
            (Stage::Done, _) => return Ok(self.finished()),
            (Stage::NotStarted, _) => return Err(Error::Parse("seq")),
            (Stage::Status { s, v }, input) if !matches!(input, Input::Encrypted(_)) => {
                // The charge is informative: any failure leaves it unknown.
                if let Input::Received(r) = &input {
                    self.report.soc = spa::parse_soc(r, v.ccs2).ok();
                }
                if setup.inputs.destination.is_empty() {
                    return Ok(self.finished());
                }
                let request = spa::pin_request(c, &session(&s), &setup.inputs.pin);
                (Stage::Pin { s, v }, Effect::Send(request))
            }
            (_, Input::Failed(code)) => return Err(Error::Network(code)),
            (Stage::Encrypt { .. }, Input::Encrypted(c)) if c.is_empty() => {
                return Err(Error::Parse("rsa"))
            }
            (Stage::Encrypt { device_id, kid }, Input::Encrypted(ciphertext)) => {
                let request =
                    idp::signin_request(c, &setup.inputs.username, &hex(&ciphertext), &kid);
                (Stage::Signin { device_id }, Effect::Send(request))
            }
            (_, Input::Encrypted(_)) | (Stage::Encrypt { .. }, _) => {
                return Err(Error::Parse("seq"))
            }
            (stage, Input::Received(r)) => match stage {
                Stage::Register => {
                    let device_id = spa::parse_device_id(&r)?;
                    (
                        Stage::Authorize { device_id },
                        Effect::Send(idp::authorize_request(c)),
                    )
                }
                Stage::Authorize { device_id } => {
                    idp::check_authorize(&r)?;
                    (
                        Stage::Certs { device_id },
                        Effect::Send(idp::certs_request(c)),
                    )
                }
                Stage::Certs { device_id } => {
                    let key = idp::parse_certs(&r)?;
                    let public_key_der = idp::public_key_der(&key)?;
                    (
                        Stage::Encrypt {
                            device_id,
                            kid: key.kid,
                        },
                        Effect::EncryptPassword { public_key_der },
                    )
                }
                Stage::Signin { device_id } => {
                    let code = idp::parse_signin(&r, setup.inputs.password_chars)?;
                    let request = cci::token_request(c, &signing(&device_id, 0), &code);
                    (Stage::Token { device_id }, Effect::Send(request))
                }
                Stage::Token { device_id } => {
                    let tokens = cci::parse_tokens(&r)?;
                    let request = cci::ccs_exchange_request(c, &signing(&device_id, 1), &tokens);
                    (Stage::Ccs { device_id }, Effect::Send(request))
                }
                Stage::Ccs { device_id } => {
                    let ccs = cci::parse_ccs(&r)?;
                    self.report.auth = Check::Ok;
                    let s = Auth { device_id, ccs };
                    let request = spa::vehicles_request(c, &session(&s));
                    (Stage::Vehicles { s }, Effect::Send(request))
                }
                Stage::Vehicles { s } => {
                    // One car per account is the expected case: take the first.
                    let v = spa::parse_vehicles(&r)?
                        .into_iter()
                        .next()
                        .ok_or(Error::NoVehicle)?;
                    self.report.vehicle = Check::Ok;
                    let request = spa::status_request(c, &session(&s), &v);
                    (Stage::Status { s, v }, Effect::Send(request))
                }
                Stage::Pin { s, v } => {
                    let control = spa::parse_control_token(&r)?;
                    let request = spa::send_destination_request(
                        c,
                        &session(&s),
                        &control,
                        &v,
                        &setup.inputs.destination,
                    );
                    (Stage::Send, Effect::Send(request))
                }
                Stage::Send => {
                    spa::parse_msg_id(&r)?;
                    self.report.send = Check::Ok;
                    return Ok(self.finished());
                }
                Stage::NotStarted | Stage::Done | Stage::Encrypt { .. } | Stage::Status { .. } => {
                    unreachable!("handled above")
                }
            },
        };
        self.stage = next;
        Ok(effect)
    }

    fn fail(&mut self, phase: Phase, e: &Error) -> Effect {
        let check = Check::Ko(e.code());
        match phase {
            Phase::Auth => self.report.auth = check,
            Phase::Vehicle => self.report.vehicle = check,
            Phase::Send => self.report.send = check,
        }
        self.repair_signature = matches!(e, Error::Gate(9035 | 9036));
        self.finished()
    }

    fn finished(&mut self) -> Effect {
        self.stage = Stage::Done;
        Effect::Done(Outcome {
            report: self.report.clone(),
            repair_signature: self.repair_signature,
        })
    }
}

impl Setup {
    fn new(
        constants_json: &str,
        override_json: Option<&str>,
        inputs: Inputs,
    ) -> Result<Self, Error> {
        let mut c = AppConstants::from_json(constants_json)?;
        if let Some(text) = override_json {
            let o: FingerprintOverride =
                serde_json::from_str(text).map_err(|_| Error::Parse("override"))?;
            c = c.with_override(&o);
        }
        let e = &inputs.entropy;
        if e.len() != ENTROPY_LEN {
            return Err(Error::Parse("entropy"));
        }
        let (uuid, rest) = e.split_at(UUID_LEN);
        let (push, rest) = rest.split_at(PUSH_LEN);
        let (n0, n1) = rest.split_at(NONCE_LEN);
        Ok(Setup {
            c,
            uuid: uuid_v4(uuid),
            push_reg_id: hex(push),
            nonces: [hex(n0).to_uppercase(), hex(n1).to_uppercase()],
            inputs,
        })
    }
}

impl Setup {
    fn signing<'a>(&'a self, device_id: &'a str, nonce: usize, now_ms: u64) -> cci::Signing<'a> {
        cci::Signing {
            device_id,
            timestamp_ms: now_ms,
            request_nonce: &self.nonces[nonce],
            locale: &self.inputs.locale,
        }
    }
}

impl Auth {
    fn session(&self, now_ms: u64) -> spa::Session<'_> {
        spa::Session {
            access_token: &self.ccs.access_token,
            device_id: &self.device_id,
            now_s: now_ms / 1000,
        }
    }
}

/// Lower-case hex, as Python's `bytes.hex()`.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A random (version 4) UUID from 16 random bytes, as Python's `uuid4()`.
fn uuid_v4(bytes: &[u8]) -> String {
    let mut b = [0u8; UUID_LEN];
    b.copy_from_slice(bytes);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex(&b);
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::super::fixtures;
    use super::*;
    use serde_json::Value;

    const NOW_MS: u64 = 1_793_740_000_000;

    /// Entropy that reproduces the fixture inputs: uuid
    /// `99999999-8888-4777-8666-555555555555`, push id `ff…`, nonce
    /// `0123456789ABCDEF01234567` twice.
    fn entropy() -> Vec<u8> {
        let mut e = vec![
            0x99, 0x99, 0x99, 0x99, 0x88, 0x88, 0x47, 0x77, 0x86, 0x66, 0x55, 0x55, 0x55, 0x55,
            0x55, 0x55,
        ];
        e.extend([0xff; PUSH_LEN]);
        let nonce = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67,
        ];
        e.extend(nonce);
        e.extend(nonce);
        e
    }

    fn inputs() -> Inputs {
        let idp = fixtures::json("idp.json");
        let spa = fixtures::json("spa.json");
        Inputs {
            username: fixtures::str(&idp["signin"]["inputs"], "username").into(),
            password_chars: 12,
            pin: fixtures::str(&spa["pin"], "pin").into(),
            locale: fixtures::locale(&fixtures::json("cci.json")["inputs"]["locale"]),
            entropy: entropy(),
            destination: vec![
                serde_json::from_value(spa["send_destination"]["poi"].clone()).expect("poi"),
            ],
        }
    }

    fn run_with(inputs: Inputs) -> Run {
        Run::new(&fixtures::text("constants_synthetic.json"), None, inputs)
    }

    /// The fixture step `file`/`step` and its response case `name`.
    fn step(file: &str, step: &str) -> Value {
        fixtures::json(file)[step].clone()
    }

    fn reply(file: &str, step_name: &str, case: &str) -> Input {
        let s = step(file, step_name);
        let found = s["responses"]
            .as_array()
            .expect("responses")
            .iter()
            .find(|c| c["name"] == case)
            .unwrap_or_else(|| panic!("{file} {step_name}: no case {case}"))
            .clone();
        Input::Received(fixtures::response(&found["response"]))
    }

    fn expect_send(effect: Effect, file: &str, step_name: &str, key: &str) {
        match effect {
            Effect::Send(r) => fixtures::assert_request(&r, &step(file, step_name)[key]),
            other => panic!("{file} {step_name}: expected a request, got {other:?}"),
        }
    }

    fn done(effect: Effect) -> Outcome {
        match effect {
            Effect::Done(o) => o,
            other => panic!("expected the end of the run, got {other:?}"),
        }
    }

    /// Runs the happy path up to the CCS token; returns the vehicles request.
    fn logged_in(run: &mut Run) -> Effect {
        expect_send(run.start(NOW_MS), "spa.json", "register", "request");
        let e = run.resume(reply("spa.json", "register", "device id"), NOW_MS);
        expect_send(e, "idp.json", "authorize", "request");
        let e = run.resume(reply("idp.json", "authorize", "login page"), NOW_MS);
        expect_send(e, "idp.json", "certs", "request");
        match run.resume(reply("idp.json", "certs", "jwk"), NOW_MS) {
            Effect::EncryptPassword { public_key_der } => {
                assert_eq!(public_key_der.first(), Some(&0x30));
            }
            other => panic!("expected the password encryption, got {other:?}"),
        }
        let e = run.resume(Input::Encrypted(vec![0xab; 8]), NOW_MS);
        expect_send(e, "idp.json", "signin", "request");
        let e = run.resume(reply("idp.json", "signin", "code"), NOW_MS);
        let token_request = match e {
            Effect::Send(r) => r,
            other => panic!("expected the CCI token request, got {other:?}"),
        };
        let fixture_token = step("cci.json", "token");
        let wanted = fixtures::request(&fixture_token["request"]);
        assert_eq!(token_request.headers, wanted.headers, "token headers");
        assert!(token_request
            .url
            .ends_with("/auth/v1/token?code=CODE-123_abc.~"));
        let e = run.resume(reply("cci.json", "token", "tokens"), NOW_MS);
        expect_send(e, "cci.json", "ccs_exchange", "request");
        let e = run.resume(reply("cci.json", "ccs_exchange", "ccs token"), NOW_MS);
        expect_send(e.clone(), "spa.json", "vehicles", "request");
        e
    }

    #[test]
    fn full_run_sends_the_fixture_requests_and_ends_all_ok() {
        let mut run = run_with(inputs());
        logged_in(&mut run);
        let e = run.resume(reply("spa.json", "vehicles", "two vehicles"), NOW_MS);
        expect_send(e, "spa.json", "status", "ccs2_request");
        let e = run.resume(reply("spa.json", "status", "ccs2 soc"), NOW_MS);
        expect_send(e, "spa.json", "pin", "request");
        let e = run.resume(reply("spa.json", "pin", "control token"), NOW_MS);
        expect_send(e, "spa.json", "send_destination", "request");
        let o = done(run.resume(reply("spa.json", "send_destination", "accepted"), NOW_MS));
        assert!(o.report.all_ok(), "{}", o.report.line());
        assert_eq!(o.report.soc, Some(80));
        assert!(!o.repair_signature);
    }

    #[test]
    fn a_finished_run_stays_finished() {
        let mut run = run_with(inputs());
        logged_in(&mut run);
        run.resume(Input::Failed(-1009), NOW_MS);
        let again = done(run.resume(Input::Failed(0), NOW_MS));
        assert_eq!(again.report.vehicle, Check::Ko("net1009".into()));
        assert_eq!(done(run.start(NOW_MS)), again);
    }

    #[test]
    fn signature_rejection_asks_for_repair() {
        for (case, code, repair) in [
            ("missing fingerprint", "gate9035", true),
            ("bad signature", "gate9036", true),
            ("stale timestamp", "gate9037", false),
        ] {
            let mut run = run_with(inputs());
            run.start(NOW_MS);
            run.resume(reply("spa.json", "register", "device id"), NOW_MS);
            run.resume(reply("idp.json", "authorize", "login page"), NOW_MS);
            run.resume(reply("idp.json", "certs", "jwk"), NOW_MS);
            run.resume(Input::Encrypted(vec![0xab; 8]), NOW_MS);
            run.resume(reply("idp.json", "signin", "code"), NOW_MS);
            let o = done(run.resume(reply("cci.json", "token", case), NOW_MS));
            assert_eq!(o.report.auth, Check::Ko(code.into()), "{case}");
            assert_eq!(o.report.vehicle, Check::Pending, "{case}");
            assert_eq!(o.report.send, Check::Pending, "{case}");
            assert_eq!(o.repair_signature, repair, "{case}");
        }
    }

    #[test]
    fn login_errors_and_transport_failures_end_in_auth() {
        let mut run = run_with(inputs());
        run.start(NOW_MS);
        let o = done(run.resume(reply("spa.json", "register", "rejected"), NOW_MS));
        assert_eq!(o.report.auth, Check::Ko("4001".into()));

        let mut run = run_with(inputs());
        run.start(NOW_MS);
        run.resume(reply("spa.json", "register", "device id"), NOW_MS);
        run.resume(reply("idp.json", "authorize", "login page"), NOW_MS);
        run.resume(reply("idp.json", "certs", "jwk"), NOW_MS);
        let o = done(run.resume(Input::Encrypted(Vec::new()), NOW_MS));
        assert_eq!(
            o.report.auth,
            Check::Ko("parse-rsa".into()),
            "encryption failed"
        );

        let mut run = run_with(inputs());
        run.start(NOW_MS);
        let o = done(run.resume(Input::Failed(0), NOW_MS));
        assert_eq!(
            o.report.auth,
            Check::Ko("net".into()),
            "unknown transport error"
        );

        let mut long = inputs();
        long.password_chars = 24;
        let mut run = run_with(long);
        run.start(NOW_MS);
        run.resume(reply("spa.json", "register", "device id"), NOW_MS);
        run.resume(reply("idp.json", "authorize", "login page"), NOW_MS);
        run.resume(reply("idp.json", "certs", "jwk"), NOW_MS);
        run.resume(Input::Encrypted(vec![0xab; 8]), NOW_MS);
        let o = done(run.resume(reply("idp.json", "signin", "back to login"), NOW_MS));
        assert_eq!(o.report.auth, Check::Ko("login20".into()));
    }

    #[test]
    fn wrong_pin_fails_only_the_send() {
        let mut run = run_with(inputs());
        logged_in(&mut run);
        run.resume(reply("spa.json", "vehicles", "two vehicles"), NOW_MS);
        run.resume(reply("spa.json", "status", "ccs2 soc"), NOW_MS);
        let o = done(run.resume(reply("spa.json", "pin", "wrong pin"), NOW_MS));
        assert_eq!(o.report.auth, Check::Ok);
        assert_eq!(o.report.vehicle, Check::Ok);
        assert_eq!(o.report.soc, Some(80));
        assert_eq!(o.report.send, Check::Ko("pin".into()));
    }

    #[test]
    fn unreadable_charge_does_not_stop_the_send() {
        let mut run = run_with(inputs());
        logged_in(&mut run);
        run.resume(reply("spa.json", "vehicles", "two vehicles"), NOW_MS);
        let e = run.resume(reply("spa.json", "status", "missing"), NOW_MS);
        expect_send(e, "spa.json", "pin", "request");
        run.resume(reply("spa.json", "pin", "control token"), NOW_MS);
        let o = done(run.resume(reply("spa.json", "send_destination", "accepted"), NOW_MS));
        assert_eq!(o.report.soc, None);
        assert_eq!(o.report.send, Check::Ok);
    }

    #[test]
    fn vehicle_list_errors_and_an_empty_account() {
        let mut run = run_with(inputs());
        logged_in(&mut run);
        let o = done(run.resume(reply("spa.json", "vehicles", "expired token"), NOW_MS));
        assert_eq!(o.report.auth, Check::Ok);
        assert_eq!(o.report.vehicle, Check::Ko("token".into()));

        let mut run = run_with(inputs());
        logged_in(&mut run);
        let empty = Input::Received(Response::new(
            200,
            r#"{"retCode":"S","resCode":"0000","resMsg":{"vehicles":[]},"msgId":"m"}"#,
        ));
        let o = done(run.resume(empty, NOW_MS));
        assert_eq!(o.report.vehicle, Check::Ko("noveh".into()));
    }

    #[test]
    fn no_destination_is_a_connection_check() {
        let mut check = inputs();
        check.destination.clear();
        let mut run = run_with(check);
        logged_in(&mut run);
        run.resume(reply("spa.json", "vehicles", "two vehicles"), NOW_MS);
        let o = done(run.resume(reply("spa.json", "status", "ccs2 soc"), NOW_MS));
        assert_eq!(o.report.auth, Check::Ok);
        assert_eq!(o.report.soc, Some(80));
        assert_eq!(o.report.send, Check::Pending);
    }

    #[test]
    fn bad_setup_ends_before_any_request() {
        let mut short = inputs();
        short.entropy.pop();
        let o = done(run_with(short).start(NOW_MS));
        assert_eq!(o.report.auth, Check::Ko("parse-entropy".into()));

        let o = done(Run::new("{}", None, inputs()).start(NOW_MS));
        assert_eq!(o.report.auth, Check::Ko("parse-constants".into()));

        let constants = fixtures::text("constants_synthetic.json");
        let o = done(Run::new(&constants, Some("{"), inputs()).start(NOW_MS));
        assert_eq!(o.report.auth, Check::Ko("parse-override".into()));
    }

    #[test]
    fn a_stored_override_signs_the_cci_calls() {
        let o = FingerprintOverride {
            signing_key: super::super::fingerprint::SigningKey {
                secret: "NEWsecret".into(),
                piece: "NEWpiece".into(),
                suffix: "NEWsuffix".into(),
            },
            version: "9.9.10".into(),
            os_code: "android".into(),
            os_version: "15".into(),
            device_model: "TEST-MODEL-2".into(),
            notification_provider: "FCM".into(),
            user_agent: "okhttp/4.12.1".into(),
            region: "eu".into(),
        };
        let json = serde_json::to_string(&o).expect("override json");
        let constants = fixtures::text("constants_synthetic.json");
        let mut run = Run::new(&constants, Some(&json), inputs());
        run.start(NOW_MS);
        run.resume(reply("spa.json", "register", "device id"), NOW_MS);
        run.resume(reply("idp.json", "authorize", "login page"), NOW_MS);
        run.resume(reply("idp.json", "certs", "jwk"), NOW_MS);
        run.resume(Input::Encrypted(vec![0xab; 8]), NOW_MS);
        let Effect::Send(r) = run.resume(reply("idp.json", "signin", "code"), NOW_MS) else {
            panic!("expected the token request");
        };
        let base = fixtures::request(&step("cci.json", "token")["request"]);
        assert!(
            r.headers.iter().any(|(_, v)| v == "9.9.10"),
            "{:?}",
            r.headers
        );
        assert_ne!(
            r.header_value("X-Fingerprint"),
            base.header_value("X-Fingerprint")
        );
    }

    #[test]
    fn out_of_order_input_ends_the_run() {
        let mut run = run_with(inputs());
        run.start(NOW_MS);
        let o = done(run.resume(Input::Encrypted(vec![1]), NOW_MS));
        assert_eq!(o.report.auth, Check::Ko("parse-seq".into()));
        let o = done(run_with(inputs()).resume(Input::Failed(0), NOW_MS));
        assert_eq!(o.report.auth, Check::Ko("parse-seq".into()));
    }
}
