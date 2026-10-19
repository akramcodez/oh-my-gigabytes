//! Low-free-space notification state machine. Pure and clock-injected so oscillation, restarts and
//! clock changes are testable. Notification text carries no paths, names or cleanup targets.
use crate::{Index, now};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Normal,
    /// Below the low threshold, waiting for dwell (and cooldown) to elapse.
    Pending,
    /// Notified; stays here until free space recovers above the recovery threshold.
    Notified,
}
impl Phase {
    fn text(self) -> &'static str {
        match self {
            Phase::Normal => "normal",
            Phase::Pending => "pending",
            Phase::Notified => "notified",
        }
    }
    fn parse(value: &str) -> Phase {
        match value {
            "pending" => Phase::Pending,
            "notified" => Phase::Notified,
            _ => Phase::Normal,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State {
    pub phase: Phase,
    pub since: i64,
    pub last_notified: Option<i64>,
}
impl State {
    pub fn new(now: i64) -> Self {
        Self {
            phase: Phase::Normal,
            since: now,
            last_notified: None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub low: u64,
    pub recovery: u64,
    pub dwell_secs: i64,
    pub cooldown_secs: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Notify,
}

/// Advance one observation. A clock that moved backwards restarts dwell and cooldown instead of
/// firing early, so wall-clock changes cannot cause extra notifications.
pub fn step(state: &mut State, policy: &Policy, available: u64, now: i64) -> Action {
    if now < state.since {
        state.since = now;
    }
    if state.last_notified.is_some_and(|last| last > now) {
        state.last_notified = Some(now);
    }
    match state.phase {
        Phase::Normal => {
            if available < policy.low {
                *state = State {
                    phase: Phase::Pending,
                    since: now,
                    ..*state
                };
            }
            Action::None
        }
        Phase::Pending => {
            // Dwell requires continuously low space; touching the band above `low` re-arms.
            if available >= policy.low {
                *state = State {
                    phase: Phase::Normal,
                    since: now,
                    ..*state
                };
                return Action::None;
            }
            let dwelled = now - state.since >= policy.dwell_secs;
            let cooled = state
                .last_notified
                .is_none_or(|last| now - last >= policy.cooldown_secs);
            if dwelled && cooled {
                *state = State {
                    phase: Phase::Notified,
                    since: now,
                    last_notified: Some(now),
                };
                return Action::Notify;
            }
            Action::None
        }
        Phase::Notified => {
            if available >= policy.recovery {
                *state = State {
                    phase: Phase::Normal,
                    since: now,
                    ..*state
                };
            }
            Action::None
        }
    }
}

/// Fixed wording: only a rounded storage figure, never a path, file name or suggested deletion.
pub fn message(available: u64) -> String {
    format!(
        "Free space on a monitored volume is low ({} available). Open omg to review; nothing is cleaned automatically.",
        crate::bytes(available)
    )
}

impl Index {
    pub fn notification_state(&self, volume: &str) -> Result<State> {
        let row: Option<(String, i64, Option<i64>)> = self
            .db
            .query_row(
                "SELECT phase,since,last_notified FROM notification_state WHERE volume=?",
                [volume],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        Ok(row.map_or_else(
            || State::new(now()),
            |(phase, since, last_notified)| State {
                phase: Phase::parse(&phase),
                since,
                last_notified,
            },
        ))
    }
    pub fn save_notification_state(&self, volume: &str, state: &State) -> Result<()> {
        self.db.execute(
            "INSERT INTO notification_state VALUES(?,?,?,?) ON CONFLICT(volume) DO UPDATE SET phase=excluded.phase,since=excluded.since,last_notified=excluded.last_notified",
            params![volume, state.phase.text(), state.since, state.last_notified],
        )?;
        Ok(())
    }
}

/// Desktop delivery through a fixed, absolute `notify-send` with constant argv and message text only.
/// Returns whether a delivery attempt was started; failure never changes state-machine decisions.
pub fn deliver_desktop(text: &str) -> Result<()> {
    let program = ["/usr/bin/notify-send", "/usr/local/bin/notify-send"]
        .into_iter()
        .find(|p| std::path::Path::new(p).is_file())
        .ok_or_else(|| anyhow::anyhow!("notify-send is not installed"))?;
    deliver_with(std::path::Path::new(program), text)
}

/// The delivery itself: constant argument list, no shell, message text last after `--`. Public so
/// tests can run it against a stand-in program; production always passes a fixed absolute path.
pub fn deliver_with(program: &std::path::Path, text: &str) -> Result<()> {
    let mut child = std::process::Command::new(program)
        .args(["--app-name=omg", "--", "omg: low free space", text])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
