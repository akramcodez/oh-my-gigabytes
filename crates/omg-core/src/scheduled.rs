//! Jobs registered with the per-user scheduler: a launchd agent on macOS, a Task Scheduler task on
//! Windows, a systemd user timer on Linux. The daily scan only runs `omg scan` and never deletes.
//! The auto delete job runs `omg auto-delete run`, which deletes only under the permission the user
//! gave (decision D16) and through `cleanup.rs`. The scheduler tools are run by fixed path with
//! fixed arguments, never through a shell.
use anyhow::{Context, Result, bail};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub const LABEL: &str = "dev.omg.scan";
pub const TASK: &str = "omg\\scan";
const MARKER: &str = "Managed by omg: daily scan-only job. Remove with `omg schedule disable`.";
pub const AUTO_LABEL: &str = "dev.omg.auto-delete";
pub const AUTO_TASK: &str = "omg\\auto-delete";
const AUTO_MARKER: &str = "Managed by omg: auto delete job. Remove with `omg auto-delete off`.";
/// How often the auto delete job wakes; the run itself starts at most once in 20 hours.
pub const AUTO_EVERY_HOURS: u32 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Scan,
    AutoDelete,
}
impl Job {
    pub fn label(self) -> &'static str {
        match self {
            Job::Scan => LABEL,
            Job::AutoDelete => AUTO_LABEL,
        }
    }
    pub fn task(self) -> &'static str {
        match self {
            Job::Scan => TASK,
            Job::AutoDelete => AUTO_TASK,
        }
    }
    fn marker(self) -> &'static str {
        match self {
            Job::Scan => MARKER,
            Job::AutoDelete => AUTO_MARKER,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Spec {
    pub job: Job,
    pub executable: PathBuf,
    pub state_dir: PathBuf,
    /// Local hour of the day, 0 to 23.
    pub hour: u8,
}

/// What the job runs. Gaps are accepted (and labelled partial) because a job started by the
/// scheduler may lack permissions the terminal has; suggestions are left for the interface.
pub fn arguments(spec: &Spec) -> Result<Vec<String>> {
    if spec.hour > 23 {
        bail!("the hour must be 0 to 23");
    }
    let text = |path: &Path| -> Result<String> {
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            bail!("scheduled paths must be absolute: {}", path.display());
        }
        let text = path
            .to_str()
            .context("scheduled paths must be valid Unicode")?;
        if text.chars().any(|c| c.is_control() || c == '"') {
            bail!("unsupported character in {}", path.display());
        }
        Ok(text.to_string())
    };
    let mut out = vec![
        text(&spec.executable)?,
        "--state-dir".into(),
        text(&spec.state_dir)?,
    ];
    match spec.job {
        Job::Scan => out.extend(["scan".into(), "--accept-gaps".into(), "--no-suggest".into()]),
        Job::AutoDelete => out.extend(["auto-delete".into(), "run".into()]),
    }
    Ok(out)
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The launchd property list: low priority, output discarded so no file names land in a log.
pub fn plist(spec: &Spec) -> Result<String> {
    let arguments: String = arguments(spec)?
        .iter()
        .map(|a| format!("    <string>{}</string>\n", xml(a)))
        .collect();
    let (marker, label) = (spec.job.marker(), spec.job.label());
    // The auto delete job also runs at login and wakes every few hours, so a computer that was off
    // at the usual time still gets its run soon after it is turned on.
    let when = match spec.job {
        Job::Scan => format!(
            "  <key>StartCalendarInterval</key>\n  <dict>\n    <key>Hour</key>\n    <integer>{}</integer>\n    <key>Minute</key>\n    <integer>0</integer>\n  </dict>\n",
            spec.hour
        ),
        Job::AutoDelete => format!(
            "  <key>RunAtLoad</key>\n  <true/>\n  <key>StartInterval</key>\n  <integer>{}</integer>\n",
            AUTO_EVERY_HOURS * 3600
        ),
    };
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- {marker} -->\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>{label}</string>\n  <key>ProgramArguments</key>\n  <array>\n{arguments}  </array>\n{when}  <key>ProcessType</key>\n  <string>Background</string>\n  <key>LowPriorityIO</key>\n  <true/>\n  <key>Nice</key>\n  <integer>10</integer>\n  <key>StandardOutPath</key>\n  <string>/dev/null</string>\n  <key>StandardErrorPath</key>\n  <string>/dev/null</string>\n</dict>\n</plist>\n"
    ))
}

/// `schtasks` arguments that create the daily task for the signed-in user. Task Scheduler limits
/// the command line to 261 characters.
pub fn schtasks_create(spec: &Spec) -> Result<Vec<String>> {
    let arguments = arguments(spec)?;
    let command = arguments
        .iter()
        .map(|a| {
            if a.contains(' ') || a.contains('\\') {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if command.chars().count() > 261 {
        bail!(
            "the scan command is longer than Task Scheduler allows (261 characters); install omg in a shorter folder"
        );
    }
    let mut out = vec!["/Create".into(), "/TN".into(), spec.job.task().into()];
    match spec.job {
        Job::Scan => out.extend([
            "/SC".into(),
            "DAILY".into(),
            "/ST".into(),
            format!("{:02}:00", spec.hour),
        ]),
        Job::AutoDelete => out.extend([
            "/SC".into(),
            "HOURLY".into(),
            "/MO".into(),
            AUTO_EVERY_HOURS.to_string(),
        ]),
    }
    out.extend(["/TR".into(), command, "/F".into()]);
    Ok(out)
}

/// The executable the job points at must be a regular file; on Unix also owned by you or root and
/// not writable by others, so the job cannot be pointed at a replaceable program.
pub fn validate_executable(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path).context("executable is not accessible")?;
    if !metadata.is_file() {
        bail!("the scheduled executable must be a regular file (not a link)");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let me = rustix::process::geteuid().as_raw();
        if (metadata.uid() != me && metadata.uid() != 0) || metadata.mode() & 0o022 != 0 {
            bail!(
                "the scheduled executable must be owned by you or root and not group/world writable"
            );
        }
    }
    Ok(())
}

/// The systemd user units for the auto delete job: a one-shot service and a timer that fires at
/// login and every few hours, catching up after the computer was off.
pub fn systemd_units(spec: &Spec) -> Result<(String, String)> {
    let command = arguments(spec)?
        .iter()
        .map(|a| systemd_quote(a))
        .collect::<Vec<_>>()
        .join(" ");
    let marker = format!("# {AUTO_MARKER}");
    Ok((
        format!(
            "{marker}\n[Unit]\nDescription=OMG auto delete (only what waited in OMG Trash for the chosen period)\n\n[Service]\nType=oneshot\nExecStart={command}\nNice=10\nIOSchedulingClass=idle\nNoNewPrivileges=yes\nStandardOutput=null\nStandardError=null\n"
        ),
        format!(
            "{marker}\n[Unit]\nDescription=OMG auto delete timer\n\n[Timer]\nOnStartupSec=10min\nOnUnitActiveSec={AUTO_EVERY_HOURS}h\nPersistent=true\nOnCalendar=daily\n\n[Install]\nWantedBy=timers.target\n"
        ),
    ))
}

/// systemd command-line quoting for one argument, also neutralizing `%` specifiers and `$`.
fn systemd_quote(argument: &str) -> String {
    let mut out = String::from("\"");
    for c in argument.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '%' => out.push_str("%%"),
            '$' => out.push_str("$$"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

const SYSTEMD_UNIT: &str = "omg-auto-delete";

/// The per-user scheduler of this platform for one job, with the tool and folder it uses (tests
/// pass fakes).
pub struct Scheduler {
    pub tool: PathBuf,
    /// macOS: the LaunchAgents folder. Linux: the systemd user unit folder. Unused on Windows.
    pub agents: PathBuf,
    pub job: Job,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct State {
    pub registered: bool,
    pub detail: String,
}

impl Scheduler {
    pub fn discover(home: &Path) -> Result<Self> {
        Self::for_job(home, Job::Scan)
    }
    pub fn for_job(home: &Path, job: Job) -> Result<Self> {
        if cfg!(target_os = "macos") {
            Ok(Self {
                tool: PathBuf::from("/bin/launchctl"),
                agents: home.join("Library/LaunchAgents"),
                job,
            })
        } else if cfg!(windows) {
            let system = std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| PathBuf::from("C:\\Windows"));
            Ok(Self {
                tool: system.join("System32").join("schtasks.exe"),
                agents: PathBuf::new(),
                job,
            })
        } else if job == Job::AutoDelete {
            let tool = ["/usr/bin/systemctl", "/bin/systemctl"]
                .into_iter()
                .map(PathBuf::from)
                .find(|p| p.is_file())
                .unwrap_or_else(|| PathBuf::from("/usr/bin/systemctl"));
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(".config"));
            Ok(Self {
                tool,
                agents: config.join("systemd/user"),
                job,
            })
        } else {
            bail!("on Linux, use `omg start` for the background service");
        }
    }
    fn linux(&self) -> bool {
        !cfg!(target_os = "macos") && !cfg!(windows)
    }
    fn plist_path(&self) -> PathBuf {
        self.agents.join(format!("{}.plist", self.job.label()))
    }
    fn unit_paths(&self) -> [PathBuf; 2] {
        [
            self.agents.join(format!("{SYSTEMD_UNIT}.service")),
            self.agents.join(format!("{SYSTEMD_UNIT}.timer")),
        ]
    }
    fn run(&self, args: &[String]) -> Result<(bool, String)> {
        let mut command = Command::new(&self.tool);
        if self.linux() {
            command.arg("--user");
        }
        let output = command
            .args(args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("cannot run {}", self.tool.display()))?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Ok((output.status.success(), clip(text)))
    }
    #[cfg(unix)]
    fn domain() -> String {
        format!("gui/{}", rustix::process::getuid().as_raw())
    }
    #[cfg(not(unix))]
    fn domain() -> String {
        String::new()
    }
    fn write_ours(&self, path: &Path, content: &str) -> Result<()> {
        if path.exists() && !self.ours(path) {
            bail!(
                "{} exists and was not written by omg; leaving it alone",
                path.display()
            );
        }
        std::fs::create_dir_all(&self.agents)?;
        let name = path
            .file_name()
            .context("job file has no name")?
            .to_string_lossy();
        let staged = self.agents.join(format!(".{name}.new"));
        std::fs::write(&staged, content)?;
        std::fs::rename(&staged, path)?;
        Ok(())
    }
    pub fn enable(&self, spec: &Spec) -> Result<()> {
        validate_executable(&spec.executable)?;
        if spec.job != self.job {
            bail!("the job does not match this scheduler");
        }
        if cfg!(windows) {
            let (ok, text) = self.run(&schtasks_create(spec)?)?;
            if !ok {
                bail!("Task Scheduler refused the task: {text}");
            }
            return Ok(());
        }
        if self.linux() {
            if !self.tool.is_file() {
                bail!("systemd is not installed, so the auto delete job cannot be registered");
            }
            let (service, timer) = systemd_units(spec)?;
            let [service_path, timer_path] = self.unit_paths();
            for path in [&service_path, &timer_path] {
                if path.exists() && !self.ours(path) {
                    bail!(
                        "{} exists and was not written by omg; leaving it alone",
                        path.display()
                    );
                }
            }
            self.write_ours(&service_path, &service)?;
            self.write_ours(&timer_path, &timer)?;
            let (ok, text) = self.run(&["daemon-reload".into()])?;
            if !ok {
                bail!("systemctl --user daemon-reload failed: {text}");
            }
            let (ok, text) = self.run(&[
                "enable".into(),
                "--now".into(),
                format!("{SYSTEMD_UNIT}.timer"),
            ])?;
            if !ok {
                bail!("systemctl --user could not start the timer: {text}");
            }
            return Ok(());
        }
        let label = self.job.label();
        let path = self.plist_path();
        self.write_ours(&path, &plist(spec)?)?;
        // Replace a loaded copy so a changed hour or path takes effect.
        let _ = self.run(&["bootout".into(), format!("{}/{label}", Self::domain())]);
        let (ok, text) = self.run(&[
            "bootstrap".into(),
            Self::domain(),
            path.to_string_lossy().into_owned(),
        ])?;
        if !ok {
            bail!("launchctl could not load the job: {text}");
        }
        Ok(())
    }
    pub fn disable(&self) -> Result<bool> {
        if cfg!(windows) {
            if !self.state()?.registered {
                return Ok(false);
            }
            let (ok, text) = self.run(&[
                "/Delete".into(),
                "/TN".into(),
                self.job.task().into(),
                "/F".into(),
            ])?;
            if !ok {
                bail!("Task Scheduler could not drop the task: {text}");
            }
            return Ok(true);
        }
        if self.linux() {
            let paths = self.unit_paths();
            if !paths.iter().any(|p| p.exists()) {
                return Ok(false);
            }
            if let Some(foreign) = paths.iter().find(|p| p.exists() && !self.ours(p)) {
                bail!(
                    "{} was not written by omg; leaving it alone",
                    foreign.display()
                );
            }
            if self.tool.is_file() {
                let _ = self.run(&[
                    "disable".into(),
                    "--now".into(),
                    format!("{SYSTEMD_UNIT}.timer"),
                ]);
            }
            for path in &paths {
                if path.exists() {
                    std::fs::remove_file(path)?;
                }
            }
            if self.tool.is_file() {
                let _ = self.run(&["daemon-reload".into()]);
            }
            return Ok(true);
        }
        let path = self.plist_path();
        if !path.exists() {
            return Ok(false);
        }
        if !self.ours(&path) {
            bail!(
                "{} was not written by omg; leaving it alone",
                path.display()
            );
        }
        let label = self.job.label();
        let _ = self.run(&["bootout".into(), format!("{}/{label}", Self::domain())]);
        std::fs::remove_file(&path)?;
        Ok(true)
    }
    pub fn state(&self) -> Result<State> {
        let what = match self.job {
            Job::Scan => "daily scan",
            Job::AutoDelete => "auto delete job",
        };
        let missing = || State {
            registered: false,
            detail: "not registered".into(),
        };
        if cfg!(windows) {
            let (ok, _) = self.run(&["/Query".into(), "/TN".into(), self.job.task().into()])?;
            return Ok(State {
                registered: ok,
                detail: if ok {
                    format!("{what} registered in Task Scheduler")
                } else {
                    "not registered".into()
                },
            });
        }
        if self.linux() {
            let [service, timer] = self.unit_paths();
            if !timer.exists() {
                return Ok(missing());
            }
            let active = self.tool.is_file()
                && self
                    .run(&["is-active".into(), format!("{SYSTEMD_UNIT}.timer")])?
                    .0;
            return Ok(State {
                registered: self.ours(&timer) && self.ours(&service),
                detail: if active {
                    format!("{what} registered and its timer is active")
                } else {
                    "registered but the timer is not active; turn auto delete on again".into()
                },
            });
        }
        let path = self.plist_path();
        if !path.exists() {
            return Ok(missing());
        }
        let label = self.job.label();
        let (loaded, _) = self.run(&["print".into(), format!("{}/{label}", Self::domain())])?;
        Ok(State {
            registered: self.ours(&path),
            detail: if loaded {
                format!("{what} registered and loaded")
            } else {
                "registered but not loaded; turn it on again".into()
            },
        })
    }
    fn ours(&self, path: &Path) -> bool {
        std::fs::read_to_string(path).is_ok_and(|t| t.contains(self.job.marker()))
    }
}

/// The first 2000 bytes of a tool's output, cut on a character boundary.
fn clip(mut text: String) -> String {
    let mut end = text.len().min(2000);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_tool_output_is_cut_on_a_character_boundary() {
        let text = format!("{}\u{e9}{}", "a".repeat(1999), "b".repeat(10));
        assert_eq!(clip(text), "a".repeat(1999));
        assert_eq!(clip(" ok \n".into()), "ok");
    }

    fn spec() -> Spec {
        Spec {
            job: Job::Scan,
            executable: PathBuf::from(if cfg!(windows) {
                "C:\\Users\\a b\\omg.exe"
            } else {
                "/Users/a b/omg"
            }),
            state_dir: PathBuf::from(if cfg!(windows) {
                "C:\\Users\\a b\\AppData\\Local\\omg"
            } else {
                "/Users/a b/.local/state/omg"
            }),
            hour: 12,
        }
    }

    #[test]
    fn the_job_only_scans_and_paths_are_checked() {
        let args = arguments(&spec()).unwrap();
        assert_eq!(&args[3..], ["scan", "--accept-gaps", "--no-suggest"]);
        let mut bad = spec();
        bad.hour = 24;
        assert!(arguments(&bad).is_err());
        let mut relative = spec();
        relative.state_dir = PathBuf::from("state");
        assert!(arguments(&relative).is_err());
        let mut quoted = spec();
        quoted.executable = PathBuf::from(if cfg!(windows) {
            "C:\\a\"b\\omg.exe"
        } else {
            "/a\"b/omg"
        });
        assert!(arguments(&quoted).is_err());
    }

    #[test]
    fn plist_escapes_text_and_carries_the_marker() {
        let mut odd = spec();
        odd.executable = PathBuf::from(if cfg!(windows) {
            "C:\\a&b<c>\\omg.exe"
        } else {
            "/a&b<c>/omg"
        });
        let text = plist(&odd).unwrap();
        assert!(text.contains(MARKER));
        assert!(text.contains("a&amp;b&lt;c&gt;"));
        assert!(!text.contains("a&b"));
        assert!(text.contains("<integer>12</integer>"));
    }

    #[test]
    fn task_command_is_quoted_and_bounded() {
        let args = schtasks_create(&spec()).unwrap();
        assert_eq!(args[..3], ["/Create", "/TN", TASK]);
        let command = &args[8];
        assert!(command.starts_with('"'), "{command}");
        assert!(command.ends_with("scan --accept-gaps --no-suggest"));
        let mut long = spec();
        long.executable = PathBuf::from(format!(
            "{}{}",
            if cfg!(windows) { "C:\\" } else { "/" },
            "x".repeat(300)
        ));
        assert!(schtasks_create(&long).is_err());
    }

    #[test]
    fn auto_delete_job_runs_only_its_command_and_wakes_often() {
        let auto = Spec {
            job: Job::AutoDelete,
            ..spec()
        };
        assert_eq!(&arguments(&auto).unwrap()[3..], ["auto-delete", "run"]);
        let text = plist(&auto).unwrap();
        assert!(text.contains(AUTO_LABEL) && text.contains(AUTO_MARKER));
        assert!(text.contains("<key>RunAtLoad</key>") && text.contains("<integer>21600</integer>"));
        assert!(!text.contains("StartCalendarInterval"));
        let args = schtasks_create(&auto).unwrap();
        assert_eq!(
            args[..7],
            ["/Create", "/TN", AUTO_TASK, "/SC", "HOURLY", "/MO", "6"]
        );
        assert!(args[8].ends_with("auto-delete run"));
        let (service, timer) = systemd_units(&auto).unwrap();
        assert!(service.starts_with(&format!("# {AUTO_MARKER}")));
        assert!(service.contains("Type=oneshot") && service.contains("\"auto-delete\" \"run\""));
        assert!(timer.contains("Persistent=true") && timer.contains("OnStartupSec=10min"));
        assert_eq!(systemd_quote("a%b$c\"d"), "\"a%%b$$c\\\"d\"");
    }
}
