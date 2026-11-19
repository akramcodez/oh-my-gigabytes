use super::*;

/// A version folder: parsed version, newest change time, path.
pub(crate) type VersionDir = (Vec<u64>, i64, Vec<u8>);

#[derive(Clone, Copy)]
pub(crate) enum VersionScheme {
    Node,
    Python,
    Rust,
}

/// A development toolchain: the folders it installs into (its default and any absolute override),
/// other folders it writes to while in use, and the names of project folders its builds write (a
/// recent one means it is still used).
pub struct Toolchain {
    pub name: &'static str,
    pub roots: Vec<std::path::PathBuf>,
    pub also: Vec<std::path::PathBuf>,
    pub outputs: &'static [&'static str],
}

/// The default location plus an absolute override, without duplicates.
pub(crate) fn located(default: std::path::PathBuf, overridden: &str) -> Vec<std::path::PathBuf> {
    let mut out = vec![default];
    out.extend(absolute_env(overridden).filter(|p| *p != out[0]));
    out
}

/// Per-user toolchain installs, at each tool's documented location or absolute override.
pub fn toolchains(home: &std::path::Path) -> Vec<Toolchain> {
    let mut out = declared_toolchains(home);
    for toolchain in &mut out {
        toolchain.roots.retain(|root| tool_root_allowed(home, root));
    }
    out
}

pub(crate) fn declared_toolchains(home: &std::path::Path) -> Vec<Toolchain> {
    let gradle = located(home.join(".gradle"), "GRADLE_USER_HOME");
    let android = if cfg!(windows) {
        home.join("AppData")
            .join("Local")
            .join("Android")
            .join("Sdk")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Android/sdk")
    } else {
        home.join("Android/Sdk")
    };
    let nvm = if cfg!(windows) {
        located(home.join("AppData").join("Roaming").join("nvm"), "NVM_HOME")
    } else {
        located(home.join(".nvm"), "NVM_DIR")
    };
    let mut out = vec![
        Toolchain {
            name: "Rust (rustup)",
            roots: located(home.join(".rustup"), "RUSTUP_HOME"),
            also: located(home.join(".cargo"), "CARGO_HOME"),
            outputs: &["target"],
        },
        Toolchain {
            name: "Node.js (nvm)",
            roots: nvm,
            also: vec![home.join(".npm")],
            outputs: &["node_modules"],
        },
        Toolchain {
            name: "Python (pyenv)",
            roots: located(home.join(".pyenv"), "PYENV_ROOT"),
            also: vec![],
            outputs: &["__pycache__", ".venv", "venv"],
        },
        Toolchain {
            name: "Android SDK",
            roots: located(android, "ANDROID_HOME"),
            also: [gradle.clone(), vec![home.join(".android")]].concat(),
            outputs: &["build", ".gradle"],
        },
    ];
    if !cfg!(windows) {
        out.push(Toolchain {
            name: "Java (SDKMAN!)",
            roots: located(home.join(".sdkman"), "SDKMAN_DIR"),
            also: [gradle, vec![home.join(".m2")]].concat(),
            outputs: &["build", ".gradle", "target"],
        });
    }
    out
}

/// The product and version of a JetBrains IDE folder (`IntelliJIdea2024.1`, `PyCharmCE2023.3`).
pub(crate) fn ide_version(name: &str) -> Option<(String, Vec<u64>)> {
    let start = name.find(|c: char| c.is_ascii_digit())?;
    let (product, version) = name.split_at(start);
    if product.len() < 2 || !product.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let parts: Vec<u64> = version
        .split('.')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    (parts.len() >= 2 && parts[0] >= 2000 && parts[0] < 3000).then(|| (product.to_string(), parts))
}

/// (version, modified time in ns, path) of one installed extension folder.
pub(crate) type ExtensionVersion = (Vec<u64>, i64, Vec<u8>);

/// Splits an extension folder name into (`publisher.name`, version, platform suffix). The version is
/// the first `-` followed by three dot-separated numbers; anything else is not judged.
pub(crate) fn extension_version(name: &str) -> Option<(String, Vec<u64>, String)> {
    for (at, _) in name.match_indices('-') {
        let rest = &name[at + 1..];
        let (version, suffix) = rest.split_once('-').unwrap_or((rest, ""));
        let parts: Option<Vec<u64>> = version.split('.').map(|p| p.parse().ok()).collect();
        if let Some(parts) = parts.filter(|p| p.len() == 3)
            && at > 0
            && name[..at].contains('.')
        {
            return Some((name[..at].to_ascii_lowercase(), parts, suffix.to_string()));
        }
    }
    None
}

pub(crate) const CONDA_ENV_DAYS: i64 = 90;

impl VersionScheme {
    /// The line a version folder belongs to (only one per line is kept) and its comparable number.
    /// Anything that is not plainly a released version is not ours to judge.
    pub(crate) fn parse(self, name: &str) -> Option<(String, Vec<u64>)> {
        fn numbers(text: &str, count: usize) -> Option<Vec<u64>> {
            let parts: Vec<u64> = text
                .split('.')
                .map(|p| p.parse().ok().filter(|_| !p.is_empty()))
                .collect::<Option<_>>()?;
            (parts.len() == count).then_some(parts)
        }
        match self {
            Self::Node => {
                let version = numbers(name.strip_prefix('v')?, 3)?;
                Some((version[0].to_string(), version))
            }
            Self::Python => {
                let version = numbers(name, 3)?;
                Some((format!("{}.{}", version[0], version[1]), version))
            }
            Self::Rust => {
                let (head, rest) = name.split_once('-')?;
                if let Some(version) = numbers(head, 3) {
                    return Some((format!("release-{rest}"), version));
                }
                if head != "nightly" {
                    return None;
                }
                let mut fields = rest.splitn(4, '-');
                let (year, month, day, target) = (
                    fields.next()?,
                    fields.next()?,
                    fields.next()?,
                    fields.next()?,
                );
                if year.len() != 4 || month.len() != 2 || day.len() != 2 || target.is_empty() {
                    return None;
                }
                let date = vec![year.parse().ok()?, month.parse().ok()?, day.parse().ok()?];
                Some((format!("nightly-{target}"), date))
            }
        }
    }
}

impl Index {
    /// Toolchains not changed for `UNUSED_DAYS`, whose other folders did not change either, and with
    /// no project build folder of theirs written since. Unreadable parts mean unknown, not unused.
    pub(crate) fn unused_toolchains(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let cutoff = crate::now() - UNUSED_DAYS * 86_400;
        let gaps = self.gaps(scan)?;
        let mut out = Vec::new();
        let all = toolchains(home);
        let wanted: std::collections::HashSet<&str> =
            all.iter().flat_map(|t| t.outputs.iter().copied()).collect();
        let mut recent_outputs: Option<std::collections::HashSet<String>> = None;
        for toolchain in &all {
            for install in &toolchain.roots {
                let Some(path) = crate::rollup::relative_raw(root, install) else {
                    continue;
                };
                let Some(totals) = self.subtree(&path)? else {
                    continue;
                };
                if path.is_empty()
                    || totals.allocated < SUGGEST_MIN_BYTES as u64
                    || totals.newest <= 0
                    || totals.newest >= cutoff
                    || gaps.covers(&path)
                {
                    continue;
                }
                let mut recent = false;
                for other in &toolchain.also {
                    let Some(other) = crate::rollup::relative_raw(root, other) else {
                        continue;
                    };
                    if let Some(t) = self.subtree(&other)? {
                        recent |= t.newest >= cutoff || gaps.covers(&other);
                    }
                }
                if !recent && recent_outputs.is_none() {
                    recent_outputs = Some(self.recent_folder_names(scan, cutoff, &wanted)?);
                }
                recent = recent
                    || toolchain
                        .outputs
                        .iter()
                        .any(|name| recent_outputs.as_ref().is_some_and(|r| r.contains(*name)));
                if !recent && !out.contains(&path) {
                    out.push(path);
                }
            }
        }
        Ok(out)
    }
    /// Which of the `wanted` folder names appear anywhere in the scan changed at or after `since`,
    /// read in one pass.
    pub(crate) fn recent_folder_names(
        &self,
        scan: i64,
        since: i64,
        wanted: &std::collections::HashSet<&str>,
    ) -> Result<std::collections::HashSet<String>> {
        let mut statement = self.db.prepare_cached(
            "SELECT path,parent FROM dir_totals WHERE scan_id=? AND newest>=? AND path<>x''",
        )?;
        let mut rows = statement.query(params![scan, since])?;
        let mut found = std::collections::HashSet::new();
        while let Some(row) = rows.next()? {
            let path: Vec<u8> = row.get(0)?;
            let parent: Vec<u8> = row.get(1)?;
            let name = path.get(parent.len()..).unwrap_or_default();
            let name = String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name));
            if wanted.contains(name.as_ref()) {
                found.insert(name.into_owned());
                if found.len() == wanted.len() {
                    break;
                }
            }
        }
        Ok(found)
    }
    /// Installed versions that a newer one of the same line replaced: Node.js (nvm) within a major
    /// version, Python (pyenv) within a minor version, and Rust (rustup) numbered releases and dated
    /// nightlies per target, once the newest is a day old. Channel toolchains (`stable`, `beta`,
    /// `nightly`) are never suggested.
    /// Version pin files are contents and are not read, so an old version must also be unchanged
    /// for `policy::STALE_DAYS`; each tool installs a pinned version again on demand.
    pub(crate) fn toolchain_old_versions(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let cutoff = crate::now() - policy::STALE_DAYS * 86_400;
        let settled = crate::now() - 86_400;
        let gaps = self.gaps(scan)?;
        let mut folders: Vec<(std::path::PathBuf, VersionScheme)> = Vec::new();
        for toolchain in toolchains(home) {
            for install in toolchain.roots {
                match toolchain.name {
                    "Rust (rustup)" => {
                        folders.push((install.join("toolchains"), VersionScheme::Rust))
                    }
                    "Node.js (nvm)" if cfg!(windows) => {
                        folders.push((install, VersionScheme::Node))
                    }
                    "Node.js (nvm)" => {
                        folders.push((install.join("versions").join("node"), VersionScheme::Node))
                    }
                    "Python (pyenv)" => {
                        folders.push((install.join("versions"), VersionScheme::Python))
                    }
                    _ => {}
                }
            }
        }
        let mut statement = self.db.prepare(
            "SELECT path,newest FROM dir_totals WHERE scan_id=? AND parent=? AND path<>x''",
        )?;
        let mut out = Vec::new();
        for (folder, scheme) in folders {
            let Some(parent) = crate::rollup::relative_raw(root, &folder) else {
                continue;
            };
            if parent.is_empty() || gaps.covers(&parent) {
                continue;
            }
            let rows = statement.query_map(params![scan, parent], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Option<i64>>(1)?))
            })?;
            let mut groups: std::collections::BTreeMap<String, Vec<VersionDir>> =
                Default::default();
            for row in rows {
                let (path, newest) = row?;
                let name = crate::index::split_path(&path).1;
                let name = String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name));
                let Some((line, version)) = scheme.parse(&name) else {
                    continue;
                };
                groups
                    .entry(line)
                    .or_default()
                    .push((version, newest.unwrap_or(0), path));
            }
            for (_, mut versions) in groups {
                versions.sort();
                // A newest version installed within a day may still be on trial.
                if versions
                    .pop()
                    .is_none_or(|(_, newest, _)| newest <= 0 || newest >= settled)
                {
                    continue;
                }
                for (_, newest, path) in versions {
                    // pyenv-virtualenv keeps environments inside the version they were made with.
                    let mut envs = path.clone();
                    envs.extend_from_slice(b"/envs");
                    if newest > 0
                        && newest < cutoff
                        && !gaps.covers(&path)
                        && self.subtree(&envs)?.is_none()
                    {
                        out.push(path);
                    }
                }
            }
        }
        Ok(out)
    }
    /// Older settings and data folders of JetBrains IDEs, one per product and version
    /// (`IntelliJIdea2024.1`, `PyCharm2023.3`). The newest of each product is kept; an older one is
    /// listed once unchanged for `policy::STALE_DAYS` and the newest is a day old.
    pub(crate) fn jetbrains_old_versions(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let parents: Vec<std::path::PathBuf> = if cfg!(windows) {
            vec![
                tool_area("{data}", home).join("JetBrains"),
                tool_area("{cache}", home).join("JetBrains"),
                tool_area("{data}", home).join("Google"),
                tool_area("{cache}", home).join("Google"),
            ]
        } else if cfg!(target_os = "macos") {
            vec![
                home.join("Library/Application Support/JetBrains"),
                home.join("Library/Logs/JetBrains"),
                home.join("Library/Application Support/Google"),
                home.join("Library/Logs/Google"),
            ]
        } else {
            vec![
                tool_area("{config}", home).join("JetBrains"),
                tool_area("{data}", home).join("JetBrains"),
                tool_area("{config}", home).join("Google"),
                tool_area("{data}", home).join("Google"),
            ]
        };
        let cutoff = crate::now() - policy::STALE_DAYS * 86_400;
        let settled = crate::now() - 86_400;
        let gaps = self.gaps(scan)?;
        let mut statement = self.db.prepare(
            "SELECT path,coalesce(newest,0) FROM dir_totals WHERE scan_id=? AND parent=? AND path<>x''",
        )?;
        let mut out = Vec::new();
        for parent in parents {
            let google = parent.ends_with("Google");
            let Some(parent) = crate::rollup::relative_raw(root, &parent) else {
                continue;
            };
            if parent.is_empty() || gaps.covers(&parent) {
                continue;
            }
            let mut groups: std::collections::BTreeMap<String, Vec<VersionDir>> =
                Default::default();
            for row in statement.query_map(params![scan, parent], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?))
            })? {
                let (path, newest) = row?;
                let name = crate::index::split_path(&path).1;
                let name = String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name));
                let Some((product, version)) = ide_version(&name) else {
                    continue;
                };
                if google && product != "AndroidStudio" {
                    continue;
                }
                groups
                    .entry(product)
                    .or_default()
                    .push((version, newest, path));
            }
            for (_, mut versions) in groups {
                versions.sort();
                if versions
                    .pop()
                    .is_none_or(|(_, newest, _)| newest <= 0 || newest >= settled)
                {
                    continue;
                }
                out.extend(
                    versions
                        .into_iter()
                        .filter(|(_, newest, path)| {
                            *newest > 0 && *newest < cutoff && !gaps.covers(path)
                        })
                        .map(|(_, _, path)| path),
                );
            }
        }
        Ok(out)
    }
    /// Installed versions of the AI coding tool other than the newest and the one its launcher
    /// points at. An unreadable launcher only means the newest is the one kept.
    pub(crate) fn ai_tool_old_versions(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let directory = home.join(".local/share/claude/versions");
        let Some(parent) = crate::rollup::relative_raw(root, &directory) else {
            return Ok(vec![]);
        };
        let in_use = std::fs::read_link(home.join(".local/bin/claude")).ok();
        let mut statement = self.db.prepare(
            "SELECT CAST(parent||name AS BLOB) FROM entries WHERE scan_id=? AND parent=?",
        )?;
        let rows = statement.query_map(params![scan, parent], |r| r.get::<_, Vec<u8>>(0))?;
        let mut versions: Vec<(Vec<u64>, bool, Vec<u8>)> = Vec::new();
        for path in rows {
            let path = path?;
            let name = crate::index::split_path(&path).1;
            let name =
                String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name)).into_owned();
            // Only names that are plainly a version: anything else is not ours to judge.
            let parts: Option<Vec<u64>> = name.split('.').map(|p| p.parse().ok()).collect();
            let Some(parts) = parts else { continue };
            let used = in_use.as_ref().is_some_and(|target| {
                target
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy() == name)
            });
            versions.push((parts, used, path));
        }
        versions.sort();
        versions.pop();
        Ok(versions
            .into_iter()
            .filter(|(_, used, _)| !used)
            .map(|(_, _, path)| path)
            .collect())
    }
    /// Extension folders (`publisher.name-1.2.3`, maybe with a platform suffix) that a newer version
    /// of the same extension replaced in VS Code or Cursor. Only names; the editors' own lists are
    /// file contents and are not read. The newer version must be a day old, so the editor has had
    /// the chance to restart onto it.
    pub(crate) fn editor_old_extensions(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let settled = (crate::now() - 86_400).saturating_mul(1_000_000_000);
        let mut statement = self.db.prepare(
            "SELECT CAST(parent||name AS BLOB),kind,modified_ns FROM entries WHERE scan_id=? AND parent=?",
        )?;
        let mut out = Vec::new();
        for editor in [".vscode", ".cursor"] {
            let Some(parent) =
                crate::rollup::relative_raw(root, &home.join(editor).join("extensions"))
            else {
                continue;
            };
            let rows = statement.query_map(params![scan, parent], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<i64>>(2)?.unwrap_or(i64::MAX),
                ))
            })?;
            let mut groups: std::collections::BTreeMap<(String, String), Vec<ExtensionVersion>> =
                Default::default();
            for row in rows {
                let (path, kind, modified) = row?;
                let name = crate::index::split_path(&path).1;
                let name = String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name));
                if kind != "directory" {
                    continue;
                }
                let Some((id, version, suffix)) = extension_version(&name) else {
                    continue;
                };
                groups
                    .entry((id, suffix))
                    .or_default()
                    .push((version, modified, path));
            }
            for (_, mut versions) in groups {
                versions.sort();
                let Some((_, newest_time, _)) = versions.pop() else {
                    continue;
                };
                if newest_time < settled {
                    out.extend(versions.into_iter().map(|(_, _, path)| path));
                }
            }
        }
        Ok(out)
    }
    /// conda and mamba environments (folders holding `conda-meta`) of 50 MB or more, unchanged for
    /// `CONDA_ENV_DAYS`. An environment is made again from its list of packages, if one was kept.
    pub(crate) fn conda_envs(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let cutoff = crate::now() - CONDA_ENV_DAYS * 86_400;
        let gaps = self.gaps(scan)?;
        let mut parents: Vec<std::path::PathBuf> = [
            "miniconda3",
            "anaconda3",
            "miniforge3",
            "mambaforge",
            "micromamba",
            ".conda",
            ".mamba",
        ]
        .iter()
        .map(|base| home.join(base).join("envs"))
        .collect();
        parents.extend(absolute_env("MAMBA_ROOT_PREFIX").map(|p| p.join("envs")));
        parents.extend(absolute_env("CONDA_ENVS_PATH"));
        parents.sort();
        parents.dedup();
        let mut statement = self.db.prepare(
            "SELECT path,coalesce(newest,0),protected FROM dir_totals WHERE scan_id=? AND parent=? AND allocated>=? AND path<>x''",
        )?;
        let mut out = Vec::new();
        for parent in parents {
            if !tool_root_allowed(home, &parent) {
                continue;
            }
            let Some(parent) = crate::rollup::relative_raw(root, &parent) else {
                continue;
            };
            if parent.is_empty() {
                continue;
            }
            let rows: Vec<(Vec<u8>, i64, i64)> = statement
                .query_map(params![scan, parent, SUGGEST_MIN_BYTES], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            for (path, newest, protected) in rows {
                let mut meta = path.clone();
                meta.extend_from_slice(b"/conda-meta");
                if newest > 0
                    && newest < cutoff
                    && protected == 0
                    && !gaps.covers(&path)
                    && self.entry_at(scan, &meta)?.is_some()
                    && !out.contains(&path)
                {
                    out.push(path);
                }
            }
        }
        Ok(out)
    }
}
