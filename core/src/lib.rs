//! Sans-IO core of the IONIQ 6 HUD navigation app.
//!
//! The core never touches the network, the clock or the file system. It builds
//! requests, parses responses and makes decisions; the Swift shell performs the
//! I/O. That keeps every piece of logic testable on Linux, without a phone or a car.

pub mod bluelink;
pub mod diag;
pub mod geo;
pub mod http;

/// Version of the core, shown on the Diagnostic screen.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
