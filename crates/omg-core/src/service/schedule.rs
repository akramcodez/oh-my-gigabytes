//! Deterministic scheduling helpers: failure backoff, forward-only jitter, battery detection.
use std::path::Path;

/// Exponential backoff in seconds: `base * 2^(failures-1)`, capped at `max`.
pub fn backoff_secs(failures: u32, base_minutes: u64, max_minutes: u64) -> u64 {
    let exponent = failures.saturating_sub(1).min(20);
    base_minutes
        .saturating_mul(1u64 << exponent)
        .min(max_minutes)
        .saturating_mul(60)
}

/// Adds up to `percent` of `base` using `seed`; never schedules earlier than `base`.
pub fn jittered(base_secs: u64, percent: u64, seed: u64) -> u64 {
    let spread = base_secs / 100 * percent;
    if spread == 0 {
        return base_secs;
    }
    // SplitMix64 finalizer: well-distributed without a randomness dependency.
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    base_secs + (z ^ (z >> 31)) % (spread + 1)
}

/// `Some(true)` when a battery reports discharging, `Some(false)` when batteries are present but
/// not discharging, `None` when the platform exposes no battery information.
pub fn on_battery(power_supply: &Path) -> Option<bool> {
    let mut seen = false;
    for entry in std::fs::read_dir(power_supply).ok()?.flatten() {
        let read = |name: &str| {
            std::fs::read_to_string(entry.path().join(name))
                .map(|v| v.trim().to_owned())
                .unwrap_or_default()
        };
        if read("type") == "Battery" {
            seen = true;
            if read("status") == "Discharging" {
                return Some(true);
            }
        }
    }
    seen.then_some(false)
}
