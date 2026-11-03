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
