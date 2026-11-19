use super::*;

/// Folders whose direct subfolders each belong to one application: the per-user data, config and
/// cache areas of each platform. The defaults are always included; an absolute override is added.
pub fn app_data_parents(home: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = if cfg!(windows) {
        let mut out = vec![
            home.join("AppData").join("Local"),
            home.join("AppData").join("LocalLow"),
            home.join("AppData").join("Roaming"),
        ];
        out.extend(absolute_env("LOCALAPPDATA"));
        out.extend(absolute_env("APPDATA"));
        out
    } else if cfg!(target_os = "macos") {
        vec![home.join("Library/Application Support")]
    } else {
        let mut out = vec![
            home.join(".local/share"),
            home.join(".config"),
            home.join(".cache"),
        ];
        out.extend(absolute_env("XDG_DATA_HOME"));
        out.extend(absolute_env("XDG_CONFIG_HOME"));
        out.extend(absolute_env("XDG_CACHE_HOME"));
        out
    };
    out.retain(|parent| tool_root_allowed(home, parent));
    out.sort();
    out.dedup();
    out
}

/// Subfolders of the application areas that the system, the desktop or a VM manages, or that hold
/// installed programs, credentials or OMG's own state. Compared without regard to case.
pub(crate) const APP_DATA_SKIP: [&str; 34] = [
    // Windows
    "microsoft",
    "packages",
    "programs",
    "temp",
    "comms",
    "connecteddevicesplatform",
    "d3dscache",
    "crashdumps",
    "peerdistrepub",
    "publishers",
    "virtualstore",
    "history",
    "application data",
    "temporary internet files",
    "docker",
    "wsl",
    "lxss",
    // Linux desktop
    "trash",
    "keyrings",
    "containers",
    "flatpak",
    "systemd",
    "gnome-shell",
    "applications",
    "fonts",
    "icons",
    "dconf",
    "autostart",
    "evolution",
    // macOS
    "crashreporter",
    "knowledge",
    "fileprovider",
    "clouddocs",
    // OMG itself
    "omg",
];

/// Where applications are installed on macOS, plus one level of folders inside them.
pub(crate) fn application_folders(home: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![
        std::path::PathBuf::from("/Applications"),
        std::path::PathBuf::from("/System/Applications"),
        home.join("Applications"),
    ];
    let mut nested = Vec::new();
    for folder in &out {
        for entry in std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .take(2000)
        {
            let path = entry.path();
            if path.extension().is_none() && entry.file_type().is_ok_and(|t| t.is_dir()) {
                nested.push(path);
            }
        }
    }
    out.extend(nested);
    out
}

/// Names of the application bundles in `folders`, lower case with only letters and digits. Only
/// the folder listings are read, never a bundle's contents.
pub(crate) fn installed_apps(folders: &[std::path::PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    for folder in folders {
        for entry in std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .take(5000)
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(app) = name.strip_suffix(".app") {
                out.push(squash(app));
            }
        }
    }
    out.retain(|a| a.len() >= 3);
    out.sort();
    out.dedup();
    out
}

pub(crate) fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Whether a Library folder named `name` (lower case) may belong to an installed application. A
/// bundle identifier is matched part by part, so a vendor's other applications also count; when
/// in doubt it counts as installed.
pub(crate) fn app_installed(name: &str, apps: &[String]) -> bool {
    let name = name.strip_suffix(".savedstate").unwrap_or(name);
    let parts: Vec<String> = if name.contains('.') {
        let parts: Vec<&str> = name.split('.').collect();
        // A bundle identifier starts with a domain (`com`, `org`, `io`) or a team id or `group`.
        parts.iter().skip(1).map(|p| squash(p)).collect()
    } else {
        vec![squash(name)]
    };
    let parts: Vec<String> = parts.into_iter().filter(|p| p.len() >= 3).collect();
    if parts.is_empty() {
        return true;
    }
    parts.iter().any(|part| {
        apps.iter().any(|app| {
            app == part
                || (part.len() >= 4 && app.contains(part.as_str()))
                || (app.len() >= 4 && part.contains(app.as_str()))
        })
    })
}

pub(crate) const LEFTOVER_DAYS: i64 = 90;

impl Index {
    /// Applications' folders directly in `~/Library/Caches`, which apps must be able to recreate.
    /// Apple's own are left out: the system manages them and they come straight back.
    pub(crate) fn app_caches(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let Some(caches) = crate::rollup::relative_raw(root, &home.join("Library/Caches")) else {
            return Ok(vec![]);
        };
        let mut statement = self.db.prepare(
            "SELECT path FROM dir_totals WHERE scan_id=? AND parent=? AND allocated>=? AND path<>x''",
        )?;
        let rows = statement.query_map(params![scan, caches, SUGGEST_MIN_BYTES], |r| {
            r.get::<_, Vec<u8>>(0)
        })?;
        let mut out = Vec::new();
        for path in rows {
            let path = path?;
            let name = crate::index::split_path(&path).1;
            if !apple_cache(&String::from_utf8_lossy(
                name.strip_prefix(b"/").unwrap_or(name),
            )) {
                out.push(path);
            }
        }
        Ok(out)
    }
    /// Applications' own folders in the per-user data areas, 50 MB or larger, not changed for
    /// `UNUSED_DAYS`, with nothing protected inside (a database or credential inside means the
    /// folder is not suggested at all). Folders that hold or sit in a known cache or toolchain are
    /// left to those rules.
    pub(crate) fn unused_app_data(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let cutoff = crate::now() - UNUSED_DAYS * 86_400;
        let gaps = self.gaps(scan)?;
        let mut known: Vec<Vec<u8>> = DISCOVERIES
            .iter()
            .filter_map(|kind| discovery_root(kind, home))
            .chain(toolchains(home).into_iter().flat_map(|t| t.roots))
            .chain([home.join(".local/share/claude"), home.join(".claude")])
            .filter_map(|p| crate::rollup::relative_raw(root, &p))
            .collect();
        known.retain(|p| !p.is_empty());
        let mut statement = self.db.prepare(
            "SELECT path,newest,protected FROM dir_totals WHERE scan_id=? AND parent=? AND allocated>=? AND path<>x''",
        )?;
        let mut out: Vec<Vec<u8>> = Vec::new();
        for parent in app_data_parents(home) {
            let Some(parent) = crate::rollup::relative_raw(root, &parent) else {
                continue;
            };
            if parent.is_empty() {
                continue;
            }
            let rows = statement.query_map(params![scan, parent, SUGGEST_MIN_BYTES], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?;
            for row in rows {
                let (path, newest, protected) = row?;
                let name = crate::index::split_path(&path).1;
                let name = String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name))
                    .to_ascii_lowercase();
                let newest = newest.unwrap_or(0);
                if name.starts_with('.')
                    || name.starts_with("com.apple.")
                    || APP_DATA_SKIP.contains(&name.as_str())
                    || protected != 0
                    || newest <= 0
                    || newest >= cutoff
                    || gaps.covers(&path)
                    || known
                        .iter()
                        .any(|k| policy::inside(&path, k, true) || policy::inside(k, &path, true))
                    || out.contains(&path)
                {
                    continue;
                }
                out.push(path);
            }
        }
        Ok(out)
    }
    /// Data an application left in the per-user Library after it was removed: folders named after
    /// an application (by bundle identifier or name) that no installed application matches,
    /// unchanged for `LEFTOVER_DAYS`. Applications are known by their bundle names only.
    pub(crate) fn app_leftovers(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
        apps: &[String],
    ) -> Result<Vec<Vec<u8>>> {
        let cutoff = crate::now() - LEFTOVER_DAYS * 86_400;
        let gaps = self.gaps(scan)?;
        let mut known: Vec<Vec<u8>> = DISCOVERIES
            .iter()
            .filter_map(|kind| discovery_root(kind, home))
            .chain(toolchains(home).into_iter().flat_map(|t| t.roots))
            .filter_map(|p| crate::rollup::relative_raw(root, &p))
            .collect();
        known.retain(|p| !p.is_empty());
        let mut statement = self.db.prepare(
            "SELECT path,coalesce(newest,0),protected FROM dir_totals WHERE scan_id=? AND parent=? AND allocated>=? AND path<>x''",
        )?;
        let mut out = Vec::new();
        for area in [
            "Library/Application Support",
            "Library/Saved Application State",
            "Library/HTTPStorages",
            "Library/WebKit",
        ] {
            let Some(parent) = crate::rollup::relative_raw(root, &home.join(area)) else {
                continue;
            };
            if parent.is_empty() {
                continue;
            }
            let rows: Vec<(Vec<u8>, i64, i64)> = statement
                .query_map(params![scan, parent, 1i64 << 20], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            for (path, newest, protected) in rows {
                let name = crate::index::split_path(&path).1;
                let name = String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name))
                    .to_ascii_lowercase();
                if name.starts_with('.')
                    || name.starts_with("com.apple.")
                    || name.starts_with("group.com.apple.")
                    || APP_DATA_SKIP.contains(&name.as_str())
                    || protected != 0
                    || newest <= 0
                    || newest >= cutoff
                    || gaps.covers(&path)
                    || known
                        .iter()
                        .any(|k| policy::inside(&path, k, true) || policy::inside(k, &path, true))
                    || app_installed(&name, apps)
                {
                    continue;
                }
                out.push(path);
            }
        }
        Ok(out)
    }
    /// Logs, crash reports and temporary files the current user's programs left in the home folder:
    /// each direct item of `~/Library/Logs` (macOS) or of `%LOCALAPPDATA%\Temp` and
    /// `%LOCALAPPDATA%\CrashDumps` (Windows), 1 MB or larger and unchanged for
    /// `LOGS_MIN_AGE_DAYS`. The folders themselves are never suggested.
    pub(crate) fn home_logs_and_temp(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let folders: Vec<std::path::PathBuf> = if cfg!(target_os = "macos") {
            vec![home.join("Library/Logs")]
        } else if cfg!(windows) {
            let local = home.join("AppData").join("Local");
            vec![local.join("Temp"), local.join("CrashDumps")]
        } else {
            vec![]
        };
        let cutoff = crate::now() - LOGS_MIN_AGE_DAYS * 86_400;
        let gaps = self.gaps(scan)?;
        let mut items = self.db.prepare(
            "SELECT CAST(parent||name AS BLOB),kind,allocated,modified_ns FROM entries WHERE scan_id=? AND parent=? AND excluded IS NULL",
        )?;
        let mut out = Vec::new();
        for folder in folders {
            let Some(parent) = crate::rollup::relative_raw(root, &folder) else {
                continue;
            };
            if parent.is_empty() {
                continue;
            }
            let rows = items.query_map(params![scan, parent], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            })?;
            for row in rows {
                let (path, kind, allocated, modified) = row?;
                let (size, newest) = match kind.as_str() {
                    "directory" => match self.subtree(&path)? {
                        Some(t) => (Some(t.allocated), t.newest),
                        None => continue,
                    },
                    "file" => (
                        allocated.and_then(|a| u64::try_from(a).ok()),
                        modified.unwrap_or(0) / 1_000_000_000,
                    ),
                    _ => continue,
                };
                if size.is_some_and(|s| s >= 1 << 20)
                    && newest > 0
                    && newest < cutoff
                    && !gaps.covers(&path)
                {
                    out.push(path);
                }
            }
        }
        Ok(out)
    }
}
