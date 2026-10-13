use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub path: Vec<u8>,
    pub parent: Vec<u8>,
    pub kind: String,
    pub identity: String,
    pub apparent: u64,
    pub allocated: u64,
    pub links: u64,
    pub modified_ns: i64,
    pub changed_ns: i64,
    pub excluded: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub root: PathBuf,
    pub workers: usize,
    pub exclusions: Vec<PathBuf>,
    pub full: bool,
    /// Publish a scan that could not read some directories (labelled `partial`) instead of keeping
    /// the previous index. Off by default; explicit user choice only.
    pub publish_partial: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub id: i64,
    pub state: String,
    pub entries: u64,
    pub changed_entries: u64,
    pub errors: u64,
    pub excluded: u64,
    pub elapsed_ms: u64,
    pub workers: usize,
    pub event_queue_bound: usize,
    pub peak_pending_directories: u64,
    /// Approximate entries expected (filesystem used-inode count); an upper bound, not a promise.
    #[serde(default)]
    pub estimated_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub root: Option<Vec<u8>>,
    pub last_successful: Option<i64>,
    pub last_report: Option<ScanReport>,
    pub entries: u64,
    pub unique_apparent: u64,
    pub unique_allocated: u64,
    pub capacity: Option<u64>,
    pub available: Option<u64>,
    pub volume_sample_at: Option<i64>,
    pub scope_note: String,
}
