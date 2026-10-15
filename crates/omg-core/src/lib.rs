//! Local metadata intelligence. Filesystem mutation is not part of scanning.
pub mod capability;
pub mod catalog;
pub mod cleanup;
pub mod index;
pub mod model;
pub mod planning;
pub mod platform;
pub mod policy;
pub mod rollup;
pub mod scanner;
pub mod service;

pub use index::Index;
pub use model::{Entry, ScanOptions, ScanReport, Status};

/// Whether this build contains permanent delete at all. The analysis-only build sets this false.
pub const PERMANENT_DELETE: bool = cfg!(feature = "permanent-delete");

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Printable, quoted form of a stored path. Windows paths are stored with `/` and shown with `\`, and
/// are not Debug-escaped, so `C:\Users` stays readable.
pub fn display_path(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    if !cfg!(windows) {
        return format!("{:?}", text);
    }
    let mut out = String::from('"');
    for c in text.chars() {
        match c {
            '/' => out.push('\\'),
            '\'' => out.push('\''),
            c => out.extend(c.escape_debug()),
        }
    }
    out.push('"');
    out
}

pub fn bytes(value: u64) -> String {
    let mut amount = value as f64;
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut unit = 0;
    while amount >= 1024.0 && unit < units.len() - 1 {
        amount /= 1024.0;
        unit += 1;
    }
    format!("{amount:.2} {}", units[unit])
}
