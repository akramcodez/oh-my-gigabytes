//! A daily scan registered with the per-user scheduler of macOS (launchd agent) or Windows (Task
//! Scheduler). The job only runs `omg scan`: it never deletes and never changes OMG Trash. The
//! scheduler tools are run by fixed path with fixed arguments, never through a shell.
use anyhow::{Context, Result, bail};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub const LABEL: &str = "dev.omg.scan";
pub const TASK: &str = "omg\\scan";
const MARKER: &str = "Managed by omg: daily scan-only job. Remove with `omg schedule disable`.";

#[derive(Debug, Clone)]
pub struct Spec {
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
    Ok(vec![
        text(&spec.executable)?,
        "--state-dir".into(),
        text(&spec.state_dir)?,
        "scan".into(),
        "--accept-gaps".into(),
        "--no-suggest".into(),
    ])
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
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- {MARKER} -->\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>{LABEL}</string>\n  <key>ProgramArguments</key>\n  <array>\n{arguments}  </array>\n  <key>StartCalendarInterval</key>\n  <dict>\n    <key>Hour</key>\n    <integer>{}</integer>\n    <key>Minute</key>\n    <integer>0</integer>\n  </dict>\n  <key>ProcessType</key>\n  <string>Background</string>\n  <key>LowPriorityIO</key>\n  <true/>\n  <key>Nice</key>\n  <integer>10</integer>\n  <key>StandardOutPath</key>\n  <string>/dev/null</string>\n  <key>StandardErrorPath</key>\n  <string>/dev/null</string>\n</dict>\n</plist>\n",
        spec.hour
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
    Ok(vec![
        "/Create".into(),
        "/TN".into(),
        TASK.into(),
        "/SC".into(),
        "DAILY".into(),
        "/ST".into(),
        format!("{:02}:00", spec.hour),
        "/TR".into(),
        command,
        "/F".into(),
    ])
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

/// The per-user scheduler of this platform, with the tool and folder it uses (tests pass fakes).
pub struct Scheduler {
    pub tool: PathBuf,
    /// macOS: the LaunchAgents folder. Unused on Windows.
    pub agents: PathBuf,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct State {
    pub registered: bool,
    pub detail: String,
}

impl Scheduler {
    pub fn discover(home: &Path) -> Result<Self> {
        if cfg!(target_os = "macos") {
            Ok(Self {
                tool: PathBuf::from("/bin/launchctl"),
                agents: home.join("Library/LaunchAgents"),
            })
        } else if cfg!(windows) {
            let system = std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| PathBuf::from("C:\\Windows"));
            Ok(Self {
                tool: system.join("System32").join("schtasks.exe"),
                agents: PathBuf::new(),
            })
        } else {
            bail!("on Linux, use `omg start` for the background service");
        }
    }
    fn plist_path(&self) -> PathBuf {
        self.agents.join(format!("{LABEL}.plist"))
    }
    fn run(&self, args: &[String]) -> Result<(bool, String)> {
        let output = Command::new(&self.tool)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("cannot run {}", self.tool.display()))?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        text.truncate(2000);
        Ok((output.status.success(), text.trim().to_string()))
    }
    #[cfg(unix)]
    fn domain() -> String {
        format!("gui/{}", rustix::process::getuid().as_raw())
    }
    #[cfg(not(unix))]
    fn domain() -> String {
        String::new()
    }
    pub fn enable(&self, spec: &Spec) -> Result<()> {
        validate_executable(&spec.executable)?;
        if cfg!(windows) {
            let (ok, text) = self.run(&schtasks_create(spec)?)?;
            if !ok {
                bail!("Task Scheduler refused the task: {text}");
            }
            return Ok(());
        }
        let content = plist(spec)?;
        let path = self.plist_path();
        if path.exists() && !self.ours(&path) {
            bail!(
                "{} exists and was not written by omg; leaving it alone",
                path.display()
            );
        }
        std::fs::create_dir_all(&self.agents)?;
        let staged = self.agents.join(format!(".{LABEL}.plist.new"));
        std::fs::write(&staged, content)?;
        std::fs::rename(&staged, &path)?;
        // Replace a loaded copy so a changed hour or path takes effect.
        let _ = self.run(&["bootout".into(), format!("{}/{LABEL}", Self::domain())]);
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
            let (ok, text) =
                self.run(&["/Delete".into(), "/TN".into(), TASK.into(), "/F".into()])?;
            if !ok {
                bail!("Task Scheduler could not drop the task: {text}");
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
        let _ = self.run(&["bootout".into(), format!("{}/{LABEL}", Self::domain())]);
        std::fs::remove_file(&path)?;
        Ok(true)
    }
    pub fn state(&self) -> Result<State> {
        if cfg!(windows) {
            let (ok, _) = self.run(&["/Query".into(), "/TN".into(), TASK.into()])?;
            return Ok(State {
                registered: ok,
                detail: if ok {
                    "daily scan registered in Task Scheduler".into()
                } else {
                    "not registered".into()
                },
            });
        }
        let path = self.plist_path();
        if !path.exists() {
            return Ok(State {
                registered: false,
                detail: "not registered".into(),
            });
        }
        let (loaded, _) = self.run(&["print".into(), format!("{}/{LABEL}", Self::domain())])?;
        Ok(State {
            registered: self.ours(&path),
            detail: if loaded {
                "daily scan registered and loaded".into()
            } else {
                "registered but not loaded; run `omg schedule enable` again".into()
            },
        })
    }
    fn ours(&self, path: &Path) -> bool {
        std::fs::read_to_string(path).is_ok_and(|t| t.contains(MARKER))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> Spec {
        Spec {
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
}
