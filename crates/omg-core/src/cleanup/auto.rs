//! Auto delete (owner decision D16): with the user's standing permission, an automatic run deletes
//! what has waited in OMG Trash for the whole chosen period and has not changed since it was added.
//! Everything goes through the same plan and checks as a deletion from the interface; the
//! permission itself is the confirmation of each run's plan.
use super::{Marked, one_at_a_time};
use crate::{Index, now};
use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// The periods the user can choose, in days.
pub const PERIODS: [u32; 4] = [1, 7, 30, 180];
/// Runs start at most this often, however many times the scheduler wakes the job.
pub const MIN_RUN_GAP: i64 = 20 * 3600;
/// An item that grew by more than this, or by more than half, since it was added is skipped.
pub const GROWTH_LIMIT: u64 = 1 << 30;
const KEY: &str = "auto_delete";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub period_days: u32,
    /// Whole repositories, application data and toolchains are included too.
    pub include_risky: bool,
    /// Runs wait while a battery is discharging.
    pub plugged_in_only: bool,
    /// When the user gave permission. Without it nothing runs.
    pub consented: Option<i64>,
    pub last_run: Option<i64>,
    /// What the last run that deleted something reported, until the interface shows it.
    #[serde(default)]
    pub last_message: Option<String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            period_days: 7,
            include_risky: false,
            plugged_in_only: true,
            consented: None,
            last_run: None,
            last_message: None,
        }
    }
}
impl Settings {
    /// Whether a run may start now (it may still find nothing to do).
    pub fn active(&self) -> bool {
        self.enabled && self.consented.is_some() && PERIODS.contains(&self.period_days)
    }
    pub fn period_secs(&self) -> i64 {
        i64::from(self.period_days) * 86_400
    }
    pub fn due(&self, at: i64) -> bool {
        self.active()
            && self
                .last_run
                .is_none_or(|t| at - t >= MIN_RUN_GAP || at < t)
    }
}

/// Where an item in OMG Trash stands for the next automatic run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum Standing {
    /// Old enough and unchanged: the next run deletes it, unless the checks at deletion refuse.
    Due,
    /// Not in OMG Trash for the whole period yet; due at this time.
    Waiting(i64),
    /// The user keeps it out of automatic runs.
    Held,
    /// Repositories, application data and toolchains need the wider permission.
    NeedsPermission,
    /// It changed after it was added (newer files or much bigger); it starts waiting again.
    Changed(String),
    /// The last scan cannot tell whether it changed.
    Unknown(String),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Preview {
    pub settings: Option<Settings>,
    pub due: Vec<(i64, u64)>,
    /// The earliest time a waiting item becomes due.
    pub next: Option<i64>,
    pub waiting: usize,
    pub held: usize,
    pub excluded: usize,
}
impl Preview {
    pub fn due_bytes(&self) -> u64 {
        self.due.iter().map(|(_, b)| b).sum()
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Outcome {
    pub ran: bool,
    /// Why nothing ran (off, not due, on battery, nothing waited long enough).
    pub reason: Option<String>,
    pub deleted: usize,
    pub bytes: u64,
    /// Items left in OMG Trash this run: changed, unknown, refused at planning or at deletion.
    pub skipped: usize,
    /// The user turned auto delete off while the run was deleting.
    pub stopped: bool,
}
impl Outcome {
    fn idle(reason: &str) -> Self {
        Self {
            reason: Some(reason.into()),
            ..Default::default()
        }
    }
    /// The notification text: counts and sizes only, never a path or a name.
    pub fn message(&self) -> Option<String> {
        if self.deleted == 0 && !self.stopped {
            return None;
        }
        let mut text = format!(
            "OMG auto-deleted {} item(s), about {}.",
            self.deleted,
            crate::bytes(self.bytes)
        );
        if self.skipped > 0 {
            text.push_str(&format!(
                " {} changed or could not be checked and stayed in OMG Trash.",
                self.skipped
            ));
        }
        if self.stopped {
            text.push_str(" Stopped early because auto delete was turned off.");
        }
        Some(text)
    }
}

/// Reads the settings straight from the database file, for the watcher that stops a run the
/// moment the user turns auto delete off from another window.
pub fn enabled_in(state: &Path) -> bool {
    let Ok(db) = rusqlite::Connection::open_with_flags(
        state.join("index.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return false;
    };
    read(&db).is_ok_and(|s| s.active())
}

fn read(db: &rusqlite::Connection) -> Result<Settings> {
    let value: Option<Vec<u8>> = db
        .query_row("SELECT value FROM settings WHERE key=?", [KEY], |r| {
            r.get(0)
        })
        .optional()?;
    match value {
        None => Ok(Settings::default()),
        Some(bytes) if bytes.len() > 4096 => bail!("auto delete settings are too large"),
        Some(bytes) => serde_json::from_slice(&bytes).context("auto delete settings are invalid"),
    }
}

impl Index {
    pub fn auto_delete(&self) -> Result<Settings> {
        read(&self.db)
    }
    fn save_auto_delete(&self, settings: &Settings) -> Result<()> {
        if !PERIODS.contains(&settings.period_days) {
            bail!("choose a period of 1, 7, 30 or 180 days");
        }
        self.db.execute(
            "INSERT INTO settings VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![KEY, serde_json::to_vec(settings)?],
        )?;
        Ok(())
    }
    /// Turn auto delete on. Call only after the user has read and accepted what it does: this is
    /// the permission every later run relies on.
    pub fn grant_auto_delete(
        &self,
        period_days: u32,
        include_risky: bool,
        plugged_in_only: bool,
    ) -> Result<Settings> {
        if !crate::PERMANENT_DELETE {
            bail!("this build cannot delete, so auto delete is not available");
        }
        let settings = Settings {
            consented: Some(now()),
            enabled: true,
            period_days,
            include_risky,
            plugged_in_only,
            ..self.auto_delete()?
        };
        self.save_auto_delete(&settings)?;
        Ok(settings)
    }
    /// Turn auto delete off and withdraw the permission. A run in progress stops before its next item.
    pub fn revoke_auto_delete(&self) -> Result<Settings> {
        let settings = Settings {
            enabled: false,
            consented: None,
            ..self.auto_delete()?
        };
        self.save_auto_delete(&settings)?;
        Ok(settings)
    }
    /// Changes that need no new permission: a different period, or the battery rule. Widening to
    /// repositories, application data and toolchains goes through `grant_auto_delete`.
    pub fn tune_auto_delete(
        &self,
        period_days: Option<u32>,
        plugged_in_only: Option<bool>,
        narrow: bool,
    ) -> Result<Settings> {
        let mut settings = self.auto_delete()?;
        if let Some(days) = period_days {
            settings.period_days = days;
        }
        if let Some(on) = plugged_in_only {
            settings.plugged_in_only = on;
        }
        if narrow {
            settings.include_risky = false;
        }
        self.save_auto_delete(&settings)?;
        Ok(settings)
    }
    /// Keep an item in OMG Trash out of automatic runs, or let it back in.
    pub fn hold(&self, id: i64, keep: bool) -> Result<()> {
        if self
            .db
            .execute("UPDATE marked SET hold=? WHERE id=?", params![keep, id])?
            == 0
        {
            bail!("item {id} is not in OMG Trash");
        }
        Ok(())
    }
    /// Paths of findings that need the wider permission beyond whole repositories and application
    /// data, which `one_at_a_time` decides: toolchains.
    fn toolchain_paths(&self, home: &Path) -> Result<Vec<Vec<u8>>> {
        Ok(self
            .findings_in(1000, 0, false, home)?
            .into_iter()
            .filter(|f| f.rule.category == "unused_toolchains")
            .map(|f| f.entry.path)
            .collect())
    }
    fn standing_of(
        &self,
        item: &Marked,
        settings: &Settings,
        at: i64,
        root: &Path,
        home: &Path,
        toolchains: &[Vec<u8>],
    ) -> Result<Standing> {
        if item.hold {
            return Ok(Standing::Held);
        }
        let ready = item.added.saturating_add(settings.period_secs());
        if at < ready {
            return Ok(Standing::Waiting(ready));
        }
        let Some(entry) = self.entry(&item.path)? else {
            return Ok(Standing::Unknown("not in the last scan".into()));
        };
        let (bytes, newest) = if entry.kind == "directory" {
            match self.subtree(&item.path)? {
                Some(t) if !t.incomplete && t.newest > 0 => (t.allocated, t.newest),
                _ => {
                    return Ok(Standing::Unknown(
                        "the last scan could not read all of it".into(),
                    ));
                }
            }
        } else {
            (entry.allocated, entry.modified_ns.div_euclid(1_000_000_000))
        };
        if newest > item.added {
            return Ok(Standing::Changed("changed after it was added".into()));
        }
        let base = item.added_bytes;
        if bytes > base.saturating_add(base / 2) || bytes > base.saturating_add(GROWTH_LIMIT) {
            return Ok(Standing::Changed("grew a lot after it was added".into()));
        }
        if !settings.include_risky {
            let verdict = self.verdict_for(&entry, home)?;
            if one_at_a_time(root, home, &entry, verdict.kind) || toolchains.contains(&item.path) {
                return Ok(Standing::NeedsPermission);
            }
        }
        Ok(Standing::Due)
    }
    /// Where each item in OMG Trash stands for automatic runs, against the published index.
    pub fn auto_standing(&self, home: &Path) -> Result<Vec<(Marked, Standing)>> {
        let settings = self.auto_delete()?;
        let Ok((_, root, _)) = self.scope() else {
            return Ok(vec![]);
        };
        let toolchains = if settings.include_risky {
            vec![]
        } else {
            self.toolchain_paths(home)?
        };
        let at = now();
        let mut out = Vec::new();
        for item in self.marked()? {
            let standing = self.standing_of(&item, &settings, at, &root, home, &toolchains)?;
            out.push((item, standing));
        }
        Ok(out)
    }
    /// What the next automatic run would take, for the interface and `omg auto-delete status`.
    pub fn auto_preview(&self, home: &Path) -> Result<Preview> {
        let settings = self.auto_delete()?;
        let mut preview = Preview {
            settings: Some(settings.clone()),
            ..Default::default()
        };
        if !settings.active() {
            return Ok(preview);
        }
        for (item, standing) in self.auto_standing(home)? {
            match standing {
                Standing::Due => preview.due.push((item.id, item.allocated)),
                Standing::Waiting(at) => {
                    preview.waiting += 1;
                    preview.next = Some(preview.next.map_or(at, |n: i64| n.min(at)));
                }
                Standing::Held => preview.held += 1,
                _ => preview.excluded += 1,
            }
        }
        Ok(preview)
    }
    /// Whether anything in OMG Trash has waited the whole period, so a run is worth a scan.
    pub fn auto_worth_scanning(&self) -> Result<bool> {
        let settings = self.auto_delete()?;
        let at = now();
        Ok(settings.due(at)
            && self
                .marked()?
                .iter()
                .any(|m| !m.hold && m.added.saturating_add(settings.period_secs()) <= at))
    }
    /// The report of the last run that deleted something, once: the interface shows it at launch.
    pub fn auto_take_message(&self) -> Result<Option<String>> {
        let mut settings = self.auto_delete()?;
        let message = settings.last_message.take();
        if message.is_some() {
            self.save_auto_delete(&settings)?;
        }
        Ok(message)
    }
    /// Mark that the run took place even though it deleted nothing, so the job does not retry.
    pub fn auto_note_run(&self) -> Result<()> {
        let mut settings = self.auto_delete()?;
        settings.last_run = Some(now());
        self.save_auto_delete(&settings)
    }
    /// One automatic run against the published index (scan first). Items that changed start their
    /// wait again; due items go through `prepare_delete`'s checks and `delete_with_control`. A
    /// watcher stops the run before the next item once the user turns auto delete off.
    pub fn auto_run(&mut self, home: &Path) -> Result<Outcome> {
        let settings = self.auto_delete()?;
        if !settings.active() {
            return Ok(Outcome::idle("auto delete is off"));
        }
        let at = now();
        if !settings.due(at) {
            return Ok(Outcome::idle("ran less than 20 hours ago"));
        }
        let mut outcome = Outcome {
            ran: true,
            ..Default::default()
        };
        let mut due = Vec::new();
        for (item, standing) in self.auto_standing(home)? {
            match standing {
                Standing::Due => due.push(item.id),
                Standing::Changed(_) => {
                    outcome.skipped += 1;
                    let size = match self.subtree(&item.path)? {
                        Some(t) => t.allocated,
                        None => self.entry(&item.path)?.map_or(0, |e| e.allocated),
                    };
                    self.db.execute(
                        "UPDATE marked SET added=?,added_bytes=?,allocated=? WHERE id=?",
                        params![at, i64::try_from(size)?, i64::try_from(size)?, item.id],
                    )?;
                }
                Standing::Unknown(_) => outcome.skipped += 1,
                _ => (),
            }
        }
        self.auto_note_run()?;
        if due.is_empty() {
            outcome.reason = Some("nothing has waited the whole period unchanged".into());
            return Ok(outcome);
        }
        let (scan, items, denied, _) = self.classify(&due, home, !settings.include_risky)?;
        outcome.skipped += denied.len();
        if items.is_empty() {
            return Ok(outcome);
        }
        let plan = self.issue(scan, items, vec![], vec![], due)?;
        let stop = AtomicBool::new(false);
        let finished = AtomicBool::new(false);
        let state = self.state.clone();
        let report = std::thread::scope(|scope| {
            scope.spawn(|| {
                while !finished.load(Ordering::SeqCst) {
                    if !enabled_in(&state) {
                        stop.store(true, Ordering::SeqCst);
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
            });
            self.delete_source = "auto";
            let report = self.delete_with_control(
                &plan,
                &plan.fingerprint.clone(),
                home,
                Some(&stop),
                |_| {},
            );
            self.delete_source = "files";
            finished.store(true, Ordering::SeqCst);
            report
        })?;
        outcome.deleted = report.deleted.len();
        outcome.bytes = plan
            .items
            .iter()
            .filter(|i| report.deleted.contains(&i.path))
            .map(|i| i.allocated)
            .sum();
        outcome.stopped = report.cancelled;
        outcome.skipped += report
            .skipped
            .iter()
            .filter(|(_, why)| !why.starts_with("cancelled"))
            .count();
        if let Some(message) = outcome.message() {
            let mut settings = self.auto_delete()?;
            settings.last_message = Some(message);
            self.save_auto_delete(&settings)?;
        }
        Ok(outcome)
    }
}
