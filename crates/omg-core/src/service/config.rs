//! Strict, bounded service configuration. Nothing here can name a command, a path to remove or an
//! approval; unknown keys are rejected rather than ignored.
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{io::Read, path::Path};

pub const MAX_CONFIG: u64 = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceConfig {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub schedule: Schedule,
    #[serde(default)]
    pub resources: Resources,
    #[serde(default)]
    pub notifications: Notifications,
    #[serde(default)]
    pub retention: Retention,
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Schedule {
    pub interval_minutes: u64,
    pub reconcile_hours: u64,
    pub jitter_percent: u64,
    pub pause_on_battery: bool,
    pub backoff_base_minutes: u64,
    pub backoff_max_minutes: u64,
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Resources {
    pub workers: usize,
    pub nice: i32,
    pub volume_check_minutes: u64,
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Notifications {
    pub enabled: bool,
    pub desktop: bool,
    pub low_free_bytes: u64,
    pub recovery_free_bytes: u64,
    pub dwell_minutes: u64,
    pub cooldown_hours: u64,
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Retention {
    pub sample_days: u64,
    pub max_samples: u64,
}

fn version() -> u32 {
    1
}
impl Default for Schedule {
    fn default() -> Self {
        Self {
            interval_minutes: 360,
            reconcile_hours: 168,
            jitter_percent: 10,
            pause_on_battery: true,
            backoff_base_minutes: 15,
            backoff_max_minutes: 1440,
        }
    }
}
impl Default for Resources {
    fn default() -> Self {
        Self {
            workers: 1,
            nice: 10,
            volume_check_minutes: 5,
        }
    }
}
impl Default for Notifications {
    fn default() -> Self {
        Self {
            enabled: false,
            desktop: false,
            low_free_bytes: 0,
            recovery_free_bytes: 0,
            dwell_minutes: 10,
            cooldown_hours: 24,
        }
    }
}
impl Default for Retention {
    fn default() -> Self {
        Self {
            sample_days: 365,
            max_samples: 4000,
        }
    }
}
impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            version: 1,
            schedule: Schedule::default(),
            resources: Resources::default(),
            notifications: Notifications::default(),
            retention: Retention::default(),
        }
    }
}

impl ServiceConfig {
    /// A missing file means defaults (notifications off). Symlinks, special files and oversize files are refused.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error.into()),
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                bail!("configuration must be a regular file, not a link or special file")
            }
            Ok(_) => (),
        }
        let mut text = String::new();
        std::fs::File::open(path)?
            .take(MAX_CONFIG + 1)
            .read_to_string(&mut text)
            .context("configuration is unreadable or not UTF-8")?;
        Self::parse(&text)
    }
    pub fn parse(text: &str) -> Result<Self> {
        if text.len() as u64 > MAX_CONFIG {
            bail!("configuration exceeds {MAX_CONFIG} bytes");
        }
        let config: Self = toml::from_str(text).context("invalid configuration")?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        let (s, r, n, t) = (
            &self.schedule,
            &self.resources,
            &self.notifications,
            &self.retention,
        );
        if self.version != 1 {
            bail!("unsupported configuration version {}", self.version);
        }
        if !(15..=10_080).contains(&s.interval_minutes) {
            bail!("schedule.interval_minutes must be 15..=10080");
        }
        if !(1..=8_760).contains(&s.reconcile_hours) {
            bail!("schedule.reconcile_hours must be 1..=8760");
        }
        if s.jitter_percent > 50 {
            bail!("schedule.jitter_percent must be 0..=50");
        }
        if !(1..=1_440).contains(&s.backoff_base_minutes)
            || s.backoff_max_minutes < s.backoff_base_minutes
            || s.backoff_max_minutes > 10_080
        {
            bail!("schedule backoff must satisfy 1 <= base <= max <= 10080 minutes");
        }
        if !(1..=2).contains(&r.workers) {
            bail!("resources.workers must be 1..=2 for the background service");
        }
        if !(0..=19).contains(&r.nice) {
            bail!("resources.nice must be 0..=19 (never raises priority)");
        }
        if !(1..=60).contains(&r.volume_check_minutes) {
            bail!("resources.volume_check_minutes must be 1..=60");
        }
        if n.enabled {
            if n.low_free_bytes == 0 {
                bail!("notifications.low_free_bytes must be positive when enabled");
            }
            if n.recovery_free_bytes <= n.low_free_bytes {
                bail!("notifications.recovery_free_bytes must exceed low_free_bytes (hysteresis)");
            }
            if !(1..=1_440).contains(&n.dwell_minutes) || !(1..=8_760).contains(&n.cooldown_hours) {
                bail!("notification dwell must be 1..=1440 minutes and cooldown 1..=8760 hours");
            }
        }
        if !(1..=3_650).contains(&t.sample_days) || !(10..=100_000).contains(&t.max_samples) {
            bail!("retention.sample_days must be 1..=3650 and max_samples 10..=100000");
        }
        Ok(())
    }
}
