//! Optional background service (Phase 5). Scan-only by construction: nothing in this module
//! references cleanup planning, authorization or Trash handling.
pub mod config;
#[cfg(target_os = "linux")]
pub mod daemon;
#[cfg(target_os = "linux")]
pub mod ipc;
pub mod notify;
pub mod schedule;
pub mod trend;
#[cfg(target_os = "linux")]
pub mod unit;

pub use config::ServiceConfig;
