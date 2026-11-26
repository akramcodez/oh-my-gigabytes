//! Desktop notifications and power state for the auto delete job. Notification text is counts and
//! sizes only (S12), and every tool is run by fixed path with fixed arguments, never a shell.
use anyhow::{Result, bail};
use std::process::{Command, Stdio};

/// Show `text` as a desktop notification titled OMG. Fails when the platform tool is missing.
pub fn notify(text: &str) -> Result<()> {
    if text.chars().any(|c| c.is_control()) || text.len() > 400 {
        bail!("notification text must be one short line");
    }
    let mut command = notifier(text)?;
    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        bail!("the notification tool failed");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn notifier(text: &str) -> Result<Command> {
    let mut command = Command::new("/usr/bin/osascript");
    command.args([
        "-e",
        "on run argv",
        "-e",
        "display notification (item 1 of argv) with title \"OMG\"",
        "-e",
        "end run",
        text,
    ]);
    Ok(command)
}

#[cfg(windows)]
fn notifier(text: &str) -> Result<Command> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // PowerShell's own app id lets a toast show without registering one for omg. The text arrives
    // in an environment variable, so it is never parsed as script.
    const SCRIPT: &str = "$null=[Windows.UI.Notifications.ToastNotificationManager,Windows.UI.Notifications,ContentType=WindowsRuntime];$x=[Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02);$t=$x.GetElementsByTagName('text');$null=$t.Item(0).AppendChild($x.CreateTextNode('OMG'));$null=$t.Item(1).AppendChild($x.CreateTextNode($env:OMG_NOTE));[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe').Show([Windows.UI.Notifications.ToastNotification]::new($x))";
    let system = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| std::path::PathBuf::from("C:\\Windows"));
    let mut command =
        Command::new(system.join("System32\\WindowsPowerShell\\v1.0\\powershell.exe"));
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            SCRIPT,
        ])
        .env("OMG_NOTE", text)
        .creation_flags(CREATE_NO_WINDOW);
    Ok(command)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn notifier(text: &str) -> Result<Command> {
    let program = ["/usr/bin/notify-send", "/usr/local/bin/notify-send"]
        .into_iter()
        .find(|p| std::path::Path::new(p).is_file())
        .ok_or_else(|| anyhow::anyhow!("notify-send is not installed"))?;
    let mut command = Command::new(program);
    command.args(["--app-name=omg", "--", "OMG", text]);
    Ok(command)
}

/// Hide the console window Windows opens for a job started by Task Scheduler. A console shared
/// with a terminal the user opened is left alone.
#[cfg(windows)]
pub fn hide_console() {
    use windows_sys::Win32::System::Console::{GetConsoleProcessList, GetConsoleWindow};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};
    let mut processes = [0u32; 2];
    // SAFETY: the buffer holds the two entries the call is told about; the window handle is only
    // passed back to the system.
    unsafe {
        if GetConsoleProcessList(processes.as_mut_ptr(), 2) == 1 {
            let window = GetConsoleWindow();
            if !window.is_null() {
                ShowWindow(window, SW_HIDE);
            }
        }
    }
}
#[cfg(not(windows))]
pub fn hide_console() {}

/// `Some(true)` while a battery is discharging, `Some(false)` on mains power, `None` when unknown.
pub fn on_battery() -> Option<bool> {
    power()
}

#[cfg(target_os = "macos")]
fn power() -> Option<bool> {
    let output = Command::new("/usr/bin/pmset")
        .args(["-g", "batt"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    pmset_on_battery(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(windows)]
fn power() -> Option<bool> {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    // SAFETY: the pointer is to a live, writable struct of the expected type.
    if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
        return None;
    }
    match status.ACLineStatus {
        0 => Some(true),
        1 => Some(false),
        _ => None,
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn power() -> Option<bool> {
    crate::service::schedule::on_battery(std::path::Path::new("/sys/class/power_supply"))
}

/// Reads `pmset -g batt` output: its first line names the source the computer draws from.
pub fn pmset_on_battery(output: &str) -> Option<bool> {
    let first = output.lines().next()?;
    if first.contains("'Battery Power'") {
        Some(true)
    } else if first.contains("'AC Power'") {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pmset_output_is_read_from_its_first_line() {
        assert_eq!(
            pmset_on_battery(
                "Now drawing from 'Battery Power'\n -InternalBattery-0\t80%; discharging"
            ),
            Some(true)
        );
        assert_eq!(
            pmset_on_battery("Now drawing from 'AC Power'\n"),
            Some(false)
        );
        assert_eq!(pmset_on_battery(""), None);
        assert_eq!(pmset_on_battery("Now drawing from 'UPS Power'"), None);
    }

    #[test]
    fn notification_text_is_one_short_line() {
        assert!(notify("a\nb").is_err());
        assert!(notify(&"x".repeat(401)).is_err());
    }
}
