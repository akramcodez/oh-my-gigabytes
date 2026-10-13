//! Local metadata intelligence. Filesystem mutation is not part of scanning.
pub mod index;
pub mod model;
pub mod platform;
pub mod scanner;

pub use index::Index;
pub use model::{Entry, ScanOptions, ScanReport, Status};

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn display_path(raw: &[u8]) -> String {
    format!("{:?}", String::from_utf8_lossy(raw))
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
