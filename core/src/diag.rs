//! The one-line status the Diagnostic screen shows and the user copies.
//!
//! Format: `D0.1.0 auth ok · veh ok · soc 78 · send ok · plan 41 ms`.
//! A failing check shows its short reason instead of `ok`, e.g. `auth ko:401`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// Not run yet.
    Pending,
    Ok,
    /// Failed, with a short reason (an HTTP status, an error code).
    Ko(String),
}

impl Check {
    fn render(&self) -> String {
        match self {
            Check::Pending => "-".to_string(),
            Check::Ok => "ok".to_string(),
            Check::Ko(reason) => format!("ko:{}", sanitize(reason)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub auth: Check,
    pub vehicle: Check,
    /// State of charge in percent, if read.
    pub soc: Option<u8>,
    pub send: Check,
    /// Duration of the last plan computation, in milliseconds.
    pub plan_ms: Option<u32>,
}

impl Default for Report {
    fn default() -> Self {
        Self {
            auth: Check::Pending,
            vehicle: Check::Pending,
            soc: None,
            send: Check::Pending,
            plan_ms: None,
        }
    }
}

impl Report {
    pub fn line(&self) -> String {
        let soc = self.soc.map_or("-".to_string(), |s| s.to_string());
        let plan = self
            .plan_ms
            .map_or("-".to_string(), |ms| format!("{ms} ms"));
        format!(
            "D{} auth {} · veh {} · soc {} · send {} · plan {}",
            crate::VERSION,
            self.auth.render(),
            self.vehicle.render(),
            soc,
            self.send.render(),
            plan
        )
    }

    /// True when every check passed: the exit criterion of step 1.
    pub fn all_ok(&self) -> bool {
        self.auth == Check::Ok
            && self.vehicle == Check::Ok
            && self.send == Check::Ok
            && self.soc.is_some()
    }
}

/// Keep only the leading code of a reason (e.g. `401`), so no message text,
/// token or address can end up in a line the user pastes into a chat.
fn sanitize(reason: &str) -> String {
    reason
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_line() {
        let line = Report::default().line();
        assert!(line.starts_with("D"));
        assert!(
            line.ends_with("auth - · veh - · soc - · send - · plan -"),
            "{line}"
        );
    }

    #[test]
    fn all_ok_line() {
        let r = Report {
            auth: Check::Ok,
            vehicle: Check::Ok,
            soc: Some(78),
            send: Check::Ok,
            plan_ms: Some(41),
        };
        assert!(r.all_ok());
        assert!(r
            .line()
            .ends_with("auth ok · veh ok · soc 78 · send ok · plan 41 ms"));
    }

    #[test]
    fn reasons_are_short_and_clean() {
        let r = Report {
            auth: Check::Ko("401 Unauthorized: token=abc def ghi jkl".into()),
            ..Report::default()
        };
        assert!(!r.all_ok());
        let line = r.line();
        assert!(line.contains("auth ko:401 · veh"), "{line}");
        assert!(!line.contains("token"), "{line}");
    }
}
