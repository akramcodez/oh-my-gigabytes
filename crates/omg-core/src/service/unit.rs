//! Explicit per-user systemd registration. Only a user manager is ever used (no root, no system
//! units, no lingering) and `systemctl` is invoked with fixed argv, never through a shell.
use anyhow::{Context, Result, bail};
use std::{
    fs,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub const UNIT_NAME: &str = "omg.service";
const MARKER: &str =
    "# Managed by omg: scan-only background service. Remove with `omg service uninstall`.";

#[derive(Debug, Clone)]
pub struct Spec {
    pub executable: PathBuf,
    pub state_dir: PathBuf,
    pub config: Option<PathBuf>,
}

/// systemd command-line quoting for a single argument (also neutralizes `%` specifiers and `$`).
pub fn quote(argument: &str) -> Result<String> {
    if argument.chars().any(|c| c.is_control()) {
        bail!("control characters are not allowed in service arguments");
    }
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
    Ok(out)
}

fn text(path: &Path) -> Result<&str> {
    if !path.is_absolute() {
        bail!("service paths must be absolute: {}", path.display());
    }
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        bail!("parent components are not allowed in service paths");
    }
    path.to_str()
        .context("service paths must be valid UTF-8 for unit files")
}

/// The executable must be a regular file owned by the current user or root and not writable by
/// others, so registration cannot point the user manager at a replaceable binary.
pub fn validate_executable(path: &Path) -> Result<()> {
    text(path)?;
    let metadata = fs::symlink_metadata(path).context("executable is not accessible")?;
    if !metadata.is_file() {
        bail!("service executable must be a regular file (not a link)");
    }
    let me = rustix::process::geteuid().as_raw();
    if (metadata.uid() != me && metadata.uid() != 0) || metadata.mode() & 0o022 != 0 {
        bail!("service executable must be owned by you or root and not group/world writable");
    }
    if metadata.mode() & 0o111 == 0 {
        bail!("service executable is not executable");
    }
    Ok(())
}

pub fn render(spec: &Spec) -> Result<String> {
    let mut exec = vec![quote(text(&spec.executable)?)?, quote("--state-dir")?];
    exec.push(quote(text(&spec.state_dir)?)?);
    if let Some(config) = &spec.config {
        exec.push(quote("--config")?);
        exec.push(quote(text(config)?)?);
    }
    exec.extend([quote("service")?, quote("run")?]);
    Ok(format!(
        "{MARKER}\n[Unit]\nDescription=OMG scan-only storage service (never deletes files)\n\n[Service]\nType=simple\nExecStart={}\nRestart=on-failure\nRestartSec=60\nNice=10\nIOSchedulingClass=idle\nCPUWeight=20\nMemoryMax=512M\nTasksMax=64\nNoNewPrivileges=yes\nRestrictAddressFamilies=AF_UNIX\nLockPersonality=yes\n\n[Install]\nWantedBy=default.target\n",
        exec.join(" ")
    ))
}

pub fn default_unit_dir() -> Result<PathBuf> {
    Ok(std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or(crate::platform::home()?.join(".config"))
        .join("systemd/user"))
}

pub struct Systemd {
    pub systemctl: PathBuf,
    pub unit_dir: PathBuf,
    pub unit: String,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Support {
    Available,
    Unsupported(String),
}

impl Systemd {
    /// Fixed system locations only; a project directory or PATH entry is never consulted.
    pub fn discover() -> Result<Self> {
        let systemctl = ["/usr/bin/systemctl", "/bin/systemctl"]
            .into_iter()
            .map(PathBuf::from)
            .find(|p| p.is_file())
            .unwrap_or_else(|| PathBuf::from("/usr/bin/systemctl"));
        Ok(Self {
            systemctl,
            unit_dir: default_unit_dir()?,
            unit: UNIT_NAME.into(),
        })
    }
    pub fn unit_path(&self) -> PathBuf {
        self.unit_dir.join(&self.unit)
    }
    fn run(&self, args: &[&str]) -> Result<(bool, String)> {
        let output = Command::new(&self.systemctl)
            .arg("--user")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("cannot run {}", self.systemctl.display()))?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        let text: String = text
            .chars()
            .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
            .take(400)
            .collect();
        Ok((output.status.success(), text.trim().to_owned()))
    }
    fn must(&self, args: &[&str]) -> Result<String> {
        let (ok, text) = self.run(args)?;
        if !ok {
            bail!("systemctl --user {} failed: {text}", args.join(" "));
        }
        Ok(text)
    }
    /// Reports whether a user manager can be used here; "not supported" is a normal answer.
    pub fn support(&self) -> Support {
        if !self.systemctl.is_file() {
            return Support::Unsupported("systemd is not installed".into());
        }
        match self.run(&["is-system-running"]) {
            Ok((_, text)) if matches!(text.as_str(), "running" | "degraded" | "starting") => {
                Support::Available
            }
            Ok((_, text)) => Support::Unsupported(format!(
                "no usable systemd user manager (is-system-running: {})",
                if text.is_empty() { "no output" } else { &text }
            )),
            Err(error) => Support::Unsupported(format!("{error:#}")),
        }
    }
    pub fn registered(&self) -> bool {
        fs::symlink_metadata(self.unit_path()).is_ok_and(|m| m.is_file())
    }
    fn managed_by_omg(&self) -> bool {
        fs::read_to_string(self.unit_path()).is_ok_and(|t| t.starts_with(MARKER))
    }
    /// Write the unit atomically and reload. Refuses to overwrite a unit this tool did not create.
    pub fn install(&self, spec: &Spec, enable: bool) -> Result<()> {
        if let Support::Unsupported(reason) = self.support() {
            bail!("per-user service is unsupported here: {reason}");
        }
        validate_executable(&spec.executable)?;
        let contents = render(spec)?;
        if self.registered() && !self.managed_by_omg() {
            bail!(
                "{} exists but was not created by omg; refusing to overwrite it",
                self.unit_path().display()
            );
        }
        fs::create_dir_all(&self.unit_dir)?;
        let temporary = self
            .unit_dir
            .join(format!(".{}.tmp{}", self.unit, std::process::id()));
        let _ = fs::remove_file(&temporary);
        {
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o644)
                .open(&temporary)?;
            file.write_all(contents.as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(&temporary, self.unit_path())?;
        self.must(&["daemon-reload"])?;
        if enable {
            self.must(&["enable", &self.unit])?;
        }
        Ok(())
    }
    pub fn start(&self) -> Result<()> {
        if !self.registered() {
            bail!("service is not registered; run `omg service install` first");
        }
        self.must(&["start", &self.unit]).map(|_| ())
    }
    pub fn stop(&self) -> Result<()> {
        if !self.registered() {
            bail!("service is not registered");
        }
        self.must(&["stop", &self.unit]).map(|_| ())
    }
    /// Stop, disable and remove only a unit this tool created. Local state is never touched.
    pub fn uninstall(&self) -> Result<bool> {
        if !self.registered() {
            return Ok(false);
        }
        if !self.managed_by_omg() {
            bail!("refusing to remove a unit that omg did not create");
        }
        if matches!(self.support(), Support::Available) {
            let _ = self.run(&["stop", &self.unit]);
            let _ = self.run(&["disable", &self.unit]);
        }
        fs::remove_file(self.unit_path())?;
        if matches!(self.support(), Support::Available) {
            let _ = self.run(&["daemon-reload"]);
        }
        Ok(true)
    }
    /// Registration facts without changing anything. `expected` detects a moved/removed binary.
    pub fn describe(&self, expected: Option<&Spec>) -> serde_json::Value {
        let support = match self.support() {
            Support::Available => serde_json::json!("available"),
            Support::Unsupported(reason) => serde_json::json!({"unsupported": reason}),
        };
        let registered = self.registered();
        let unit_text = fs::read_to_string(self.unit_path()).unwrap_or_default();
        let active = self
            .run(&["show", &self.unit, "-p", "ActiveState", "--value"])
            .ok()
            .filter(|(ok, _)| *ok)
            .map(|(_, t)| t);
        let drift = expected
            .and_then(|spec| render(spec).ok())
            .map(|text| registered && text != unit_text);
        serde_json::json!({
            "manager": support,
            "registered": registered,
            "unit": self.unit_path().display().to_string(),
            "managed_by_omg": registered && self.managed_by_omg(),
            "systemd_active_state": active,
            "registration_differs_from_current_install": drift,
            "orphaned_binary": registered && exec_start_path(&unit_text).is_some_and(|p| !p.is_file()),
        })
    }
}

fn exec_start_path(unit: &str) -> Option<PathBuf> {
    let line = unit.lines().find_map(|l| l.strip_prefix("ExecStart=\""))?;
    let end = line.find('"')?;
    Some(PathBuf::from(
        line[..end].replace("%%", "%").replace("$$", "$"),
    ))
}
