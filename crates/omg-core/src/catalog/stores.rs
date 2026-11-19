use super::*;

/// A store search gives up on a root after listing this many entries, so a wrong pattern stays cheap.
pub(crate) const MAX_STORE_VISITS: usize = 200_000;

/// Unrecognized stores: unchanged this long, at least this big, at most this deep below an area.
pub(crate) const UNRECOGNIZED_DAYS: i64 = 30;

pub(crate) const UNRECOGNIZED_MIN_BYTES: i64 = 100 << 20;

pub(crate) const UNRECOGNIZED_MIN_FILES: u64 = 10;

pub(crate) const UNRECOGNIZED_DEPTH: usize = 4;

pub(crate) const UNRECOGNIZED_LIMIT: usize = 50;

pub(crate) const UNRECOGNIZED_NAMES: [&str; 8] = [
    "cache",
    "caches",
    "tmp",
    "temp",
    "logs",
    "log",
    "code cache",
    "gpucache",
];

pub(crate) const UNRECOGNIZED_FORMATS: [&str; 4] = ["jsonl", "ndjson", "json", "log"];

pub(crate) const MAX_STORE_ROOTS: usize = 8;

pub(crate) const MAX_STORE_PATTERNS: usize = 6;

pub(crate) const MAX_STORE_PART: usize = 128;

pub(crate) fn check_store(store: &Store) -> Result<()> {
    if store.roots.is_empty() || store.roots.len() > MAX_STORE_ROOTS {
        bail!("a store needs 1 to {MAX_STORE_ROOTS} roots");
    }
    for root in &store.roots {
        let Some((base, rest)) = root.split_once('/') else {
            bail!("store root {root:?} needs a folder below its base");
        };
        let base_ok = matches!(base, "~" | "{data}" | "{config}" | "{cache}")
            || base.strip_prefix('$').is_some_and(|name| {
                !name.is_empty()
                    && name.len() <= 64
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            });
        if !base_ok || rest.split('/').any(|p| !plain_name(p)) || rest.split('/').count() > 8 {
            bail!("invalid store root {root:?}");
        }
        // Under the home folder itself only a tool's own hidden folder can hold a store.
        if base == "~" && !rest.starts_with('.') {
            bail!("store root {root:?} is not a tool folder");
        }
    }
    if store.patterns.len() > MAX_STORE_PATTERNS
        || store.patterns.iter().any(|p| {
            p.is_empty()
                || p.len() > MAX_STORE_PART
                || p == "."
                || p == ".."
                || p.chars()
                    .any(|c| c.is_control() || "/\\:[]{}$~".contains(c))
        })
    {
        bail!("invalid store patterns");
    }
    if !["file", "directory"].contains(&store.target.as_str())
        || (store.patterns.is_empty() && store.target != "directory")
        || store.min_age_days > 3650
    {
        bail!("invalid store target or age");
    }
    Ok(())
}

/// Shell-style match of one name: `*` is any run of bytes, `?` one byte.
pub(crate) fn glob(pattern: &[u8], name: &[u8]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some((p, n));
            p += 1;
        } else if let Some((sp, sn)) = star {
            p = sp + 1;
            n = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|b| *b == b'*')
}

/// The per-user data, config and cache areas of this platform.
pub(crate) fn tool_area(base: &str, home: &std::path::Path) -> std::path::PathBuf {
    let env = |name: &str| absolute_env(name);
    if cfg!(windows) {
        let roaming = env("APPDATA").unwrap_or_else(|| home.join("AppData").join("Roaming"));
        let local = env("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local"));
        return match base {
            "{cache}" => local,
            _ => roaming,
        };
    }
    if cfg!(target_os = "macos") {
        return match base {
            "{cache}" => home.join("Library/Caches"),
            _ => home.join("Library/Application Support"),
        };
    }
    match base {
        "{cache}" => env("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache")),
        "{config}" => env("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")),
        _ => env("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share")),
    }
}

/// A store root resolved on this machine, or `None` when it does not apply here. Even a root taken
/// from an environment variable must be inside the home folder and a tool's own folder.
pub fn store_root(template: &str, home: &std::path::Path) -> Option<std::path::PathBuf> {
    let (base, rest) = template.split_once('/')?;
    let start = match base {
        "~" => home.to_path_buf(),
        "{data}" | "{config}" | "{cache}" => tool_area(base, home),
        _ => absolute_env(base.strip_prefix('$')?)?,
    };
    let root = rest.split('/').fold(start, |path, part| path.join(part));
    (root.starts_with(home) && tool_root_allowed(home, &root) && root != home).then_some(root)
}

/// The root template that names `path` on any machine: relative to the closest tool area, or to
/// the home folder when the first folder below it is a hidden tool folder.
pub(crate) fn store_template(path: &std::path::Path, home: &std::path::Path) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for base in ["{cache}", "{config}", "{data}"] {
        let area = tool_area(base, home);
        if let Ok(rest) = path.strip_prefix(&area)
            && rest.components().count() >= 1
            && best
                .as_ref()
                .is_none_or(|(len, _)| area.as_os_str().len() > *len)
        {
            let rest = rest.to_str()?.replace('\\', "/");
            best = Some((area.as_os_str().len(), format!("{base}/{rest}")));
        }
    }
    if let Some((_, template)) = best {
        return Some(template);
    }
    let rest = path.strip_prefix(home).ok()?.to_str()?.replace('\\', "/");
    rest.starts_with('.').then(|| format!("~/{rest}"))
}

pub(crate) const DRAFT_LISTING: usize = 2_000;

pub(crate) const DRAFT_FOLDERS: usize = 20;

/// Children of a folder, links skipped: (name, is_directory). Metadata only, bounded.
pub(crate) fn draft_children(path: &std::path::Path) -> Result<Vec<(String, bool)>> {
    let mut out = Vec::new();
    for item in std::fs::read_dir(path)?.take(DRAFT_LISTING) {
        let item = item?;
        let kind = item.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        if let Some(name) = item.file_name().to_str() {
            out.push((name.to_string(), kind.is_dir()));
        }
    }
    Ok(out)
}

/// The extension most of these files share, when one does.
pub(crate) fn common_extension(names: &[&str]) -> Option<String> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for name in names {
        if let Some((stem, ext)) = name.rsplit_once('.')
            && !stem.is_empty()
            && plain_name(ext)
        {
            *counts.entry(ext).or_default() += 1;
        }
    }
    let (ext, count) = counts.into_iter().max_by_key(|(_, c)| *c)?;
    (count * 2 > names.len()).then(|| ext.to_string())
}

/// A draft rule for a store folder, read from metadata and folder listings only (never a file's
/// contents). The result is checked with `parse` and is a starting point to review, not a rule
/// to trust: it still needs a source, fixtures and a signature before omg can load it.
pub fn draft_rule(path: &std::path::Path, home: &std::path::Path) -> Result<String> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || std::fs::canonicalize(path)? != path {
        bail!("give the real path of a folder, without links or relative parts");
    }
    let Some(template) = store_template(path, home) else {
        bail!("the folder is not in a tool's own area of the home folder");
    };
    let tool = template
        .trim_start_matches("~/")
        .split('/')
        .find(|p| !p.starts_with('{'))
        .unwrap_or("tool")
        .trim_start_matches('.')
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    let children = draft_children(path)?;
    let files: Vec<&str> = children
        .iter()
        .filter(|c| !c.1)
        .map(|c| c.0.as_str())
        .collect();
    let folders: Vec<&str> = children
        .iter()
        .filter(|c| c.1)
        .map(|c| c.0.as_str())
        .collect();
    let (patterns, target): (Vec<String>, &str) = if let Some(ext) = (files.len() >= folders.len())
        .then(|| common_extension(&files))
        .flatten()
    {
        (vec![format!("*.{ext}")], "file")
    } else if !folders.is_empty() {
        let mut inner = Vec::new();
        for folder in folders.iter().take(DRAFT_FOLDERS) {
            inner.extend(
                draft_children(&path.join(folder))?
                    .into_iter()
                    .filter(|c| !c.1)
                    .map(|c| c.0),
            );
        }
        let inner: Vec<&str> = inner.iter().map(String::as_str).collect();
        match common_extension(&inner) {
            Some(ext) => (vec!["*".into(), format!("*.{ext}")], "file"),
            None => (vec!["*".into()], "directory"),
        }
    } else {
        bail!("the folder holds no items that share a format");
    };
    let name = template
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let (discovery, age) = if name.contains("cache") {
        ("tool_caches", 30)
    } else if name.contains("log") {
        ("tool_logs", 7)
    } else {
        ("tool_sessions", 7)
    };
    let category = category_of(discovery).unwrap_or_default();
    let quote = |items: &[String]| {
        items
            .iter()
            .map(|i| format!("{i:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let rule = format!(
        r#"[[rules]]
id = "local.{tool}.store"
name = "Old {tool} items"
revision = 1
platforms = [{os:?}]
tool_versions = ["unknown-inventory-only"]
category = "{category}"
tier = "review"
action = "info"
discovery = "{discovery}"
explanation = "Items in {template} unchanged for {age} or more days."
side_effects = "Check what {tool} loses when one is deleted."
regeneration = "Unknown until checked against the tool's documentation."
warnings = ["Drafted from folder names only; review before use."]
preconditions = ["known_root", "metadata_only"]
exclusions = ["protected", "cloud", "active_work"]
privileges = "current-user"
failure_behavior = "informational"
compatibility = "inventory-only"
deprecated = false
fixtures = ["old-item-positive", "recent-item-near-miss"]
source = "https://example.invalid/replace-with-the-tool-documentation"

[rules.store]
roots = [{root:?}]
patterns = [{patterns}]
target = "{target}"
min_age_days = {age}
min_bytes = 1048576
"#,
        os = std::env::consts::OS,
        root = template,
        patterns = quote(&patterns),
    );
    parse(&format!(
        "schema_version = 1\nversion = 1\nengine_major = 0\n\n{rule}"
    ))
    .map_err(|e| anyhow::anyhow!("the draft did not pass the catalog checks: {e}"))?;
    Ok(rule)
}

impl Index {
    /// Items of a rule's store: each root resolved, then one level per pattern, listing only the
    /// children of folders already matched, so the cost follows the store and not the index.
    pub(crate) fn store_items(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
        store: &Store,
    ) -> Result<Vec<Vec<u8>>> {
        let cutoff = now() - i64::from(store.min_age_days) * 86_400;
        let mut children = self.db.prepare_cached(
            "SELECT name,kind FROM entries WHERE scan_id=? AND parent=? AND excluded IS NULL",
        )?;
        let mut out = Vec::new();
        let mut bases = BTreeSet::new();
        for template in &store.roots {
            let Some(base) = store_root(template, home)
                .and_then(|absolute| crate::rollup::relative_raw(root, &absolute))
                .filter(|b| !b.is_empty())
            else {
                continue;
            };
            if !bases.insert(base.clone()) {
                continue;
            }
            let mut frontier = vec![base];
            let mut visited = 0usize;
            for (level, pattern) in store.patterns.iter().enumerate() {
                let last = level + 1 == store.patterns.len();
                let mut next = Vec::new();
                for dir in &frontier {
                    let rows = children.query_map(params![scan, dir], |r| {
                        Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))
                    })?;
                    for row in rows {
                        let (name, kind) = row?;
                        visited += 1;
                        let bare = name.strip_prefix(b"/").unwrap_or(&name);
                        if (last || kind == "directory") && glob(pattern.as_bytes(), bare) {
                            let mut path = dir.clone();
                            path.extend_from_slice(&name);
                            next.push(path);
                        }
                    }
                }
                frontier = next;
                if visited > MAX_STORE_VISITS {
                    frontier.clear();
                }
            }
            for path in frontier {
                let Some(entry) = self.entry_at(scan, &path)? else {
                    continue;
                };
                if entry.kind != store.target || entry.excluded.is_some() {
                    continue;
                }
                let (bytes, newest) = if entry.kind == "directory" {
                    match self.subtree(&path)? {
                        Some(t) if !t.incomplete && t.newest > 0 => (t.allocated, t.newest),
                        _ => continue,
                    }
                } else {
                    (entry.allocated, entry.modified_ns.div_euclid(1_000_000_000))
                };
                if bytes >= store.min_bytes && newest < cutoff {
                    out.push(path);
                }
            }
        }
        Ok(out)
    }
    /// Folders in the tool areas of the home folder that look like a store no rule knows yet:
    /// named like a cache, log or temporary folder, or holding many files of one log or history
    /// format. Shown for review only; never suggested. Found by walking `dir_totals` from each
    /// area through big folders only, so small trees are never read.
    pub(crate) fn unrecognized_stores(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
        catalog: &Catalog,
        claimed: &[Vec<u8>],
    ) -> Result<Vec<Vec<u8>>> {
        let cutoff = now() - UNRECOGNIZED_DAYS * 86_400;
        let mut known: Vec<Vec<u8>> = claimed.to_vec();
        for kind in DISCOVERIES {
            if let Some(path) = discovery_root(kind, home) {
                known.extend(crate::rollup::relative_raw(root, &path));
            }
        }
        for rule in &catalog.rules {
            for template in rule.store.iter().flat_map(|s| &s.roots) {
                if let Some(path) = store_root(template, home) {
                    known.extend(crate::rollup::relative_raw(root, &path));
                }
            }
        }
        for toolchain in toolchains(home) {
            for path in toolchain.roots.iter().chain(&toolchain.also) {
                known.extend(crate::rollup::relative_raw(root, path));
            }
        }
        known.retain(|k| !k.is_empty());
        let Some(home_raw) = crate::rollup::relative_raw(root, home) else {
            return Ok(vec![]);
        };
        let mut areas: Vec<Vec<u8>> = Vec::new();
        let mut big = self.db.prepare_cached(
            "SELECT path,allocated,files,protected,coalesce(newest,0) FROM dir_totals WHERE scan_id=? AND parent=? AND allocated>=?",
        )?;
        // The hidden folders directly in the home folder, then the per-user application areas.
        let rows = big.query_map(params![scan, home_raw, UNRECOGNIZED_MIN_BYTES], |r| {
            r.get::<_, Vec<u8>>(0)
        })?;
        for path in rows {
            let path = path?;
            let name = crate::index::split_path(&path).1;
            if name.strip_prefix(b"/").unwrap_or(name).starts_with(b".") {
                areas.push(path);
            }
        }
        let mut parents: Vec<std::path::PathBuf> = app_data_parents(home);
        parents.push(tool_area("{cache}", home));
        for parent in parents {
            let Some(parent) = crate::rollup::relative_raw(root, &parent).filter(|p| !p.is_empty())
            else {
                continue;
            };
            if areas.iter().any(|a| policy::inside(&parent, a, true)) {
                continue;
            }
            let rows = big.query_map(params![scan, parent, UNRECOGNIZED_MIN_BYTES], |r| {
                r.get::<_, Vec<u8>>(0)
            })?;
            for path in rows {
                areas.push(path?);
            }
        }
        let mut files = self.db.prepare_cached(
            "SELECT name,allocated,modified_ns FROM entries WHERE scan_id=? AND parent=? AND kind='file' AND excluded IS NULL",
        )?;
        let mut out: Vec<Vec<u8>> = Vec::new();
        // Breadth first through big folders only, a few levels deep.
        let mut frontier: Vec<Vec<u8>> = areas;
        for _ in 0..UNRECOGNIZED_DEPTH {
            let mut next = Vec::new();
            for dir in frontier {
                if known
                    .iter()
                    .any(|k| policy::inside(&dir, k, true) || policy::inside(k, &dir, true))
                {
                    // A known root at or inside this folder: look below, never at the whole.
                    if !known.iter().any(|k| policy::inside(&dir, k, true)) {
                        let rows = big
                            .query_map(params![scan, dir, UNRECOGNIZED_MIN_BYTES], |r| {
                                r.get::<_, Vec<u8>>(0)
                            })?;
                        for row in rows {
                            next.push(row?);
                        }
                    }
                    continue;
                }
                let Some(totals) = self.subtree(&dir)? else {
                    continue;
                };
                if totals.protected > 0 || totals.incomplete || totals.newest == 0 {
                    let rows = big.query_map(params![scan, dir, UNRECOGNIZED_MIN_BYTES], |r| {
                        r.get::<_, Vec<u8>>(0)
                    })?;
                    for row in rows {
                        next.push(row?);
                    }
                    continue;
                }
                let name = crate::index::split_path(&dir).1;
                let name = String::from_utf8_lossy(name.strip_prefix(b"/").unwrap_or(name))
                    .to_ascii_lowercase();
                let named = UNRECOGNIZED_NAMES.contains(&name.as_str()) && totals.newest < cutoff;
                let mut by_format: std::collections::HashMap<&str, (u64, u64)> =
                    std::collections::HashMap::new();
                if !named {
                    let rows = files.query_map(params![scan, dir], |r| {
                        Ok((
                            r.get::<_, Vec<u8>>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, i64>(2)?,
                        ))
                    })?;
                    for row in rows {
                        let (file, allocated, modified) = row?;
                        if modified.div_euclid(1_000_000_000) >= cutoff {
                            continue;
                        }
                        let Some(format) = UNRECOGNIZED_FORMATS
                            .iter()
                            .find(|f| file.ends_with(format!(".{f}").as_bytes()))
                        else {
                            continue;
                        };
                        let slot = by_format.entry(format).or_default();
                        slot.0 += 1;
                        slot.1 += allocated.max(0) as u64;
                    }
                }
                let history = by_format.values().any(|(count, bytes)| {
                    *count >= UNRECOGNIZED_MIN_FILES && *bytes >= UNRECOGNIZED_MIN_BYTES as u64
                });
                if named || history {
                    out.push(dir);
                    if out.len() >= UNRECOGNIZED_LIMIT {
                        return Ok(out);
                    }
                    continue;
                }
                let rows = big.query_map(params![scan, dir, UNRECOGNIZED_MIN_BYTES], |r| {
                    r.get::<_, Vec<u8>>(0)
                })?;
                for row in rows {
                    next.push(row?);
                }
            }
            frontier = next;
        }
        Ok(out)
    }
}
