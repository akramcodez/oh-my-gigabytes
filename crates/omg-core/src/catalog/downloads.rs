use super::*;

/// A folder compared for copies: path, newest change, folded file names, bytes.
pub(crate) type FolderCopy = (Vec<u8>, i64, BTreeSet<Vec<u8>>, u64);

/// Old installers: unchanged this long, at least this big.
pub(crate) const INSTALLER_DAYS: i64 = 30;

pub(crate) const INSTALLER_MIN_BYTES: i64 = 10 << 20;

pub(crate) const INSTALLER_EXTENSIONS: [&str; 22] = [
    "dmg", "pkg", "mpkg", "xip", "iso", "msi", "msix", "exe", "appimage", "deb", "rpm", "apk",
    "ipa", "zip", "tar", "tgz", "tar.gz", "tar.xz", "tar.bz2", "7z", "rar", "vsix",
];

/// Folder copies: the size of a folder worth comparing, how many groups and files are read.
pub(crate) const COPY_MIN_BYTES: i64 = 10 << 20;

pub(crate) const COPY_PARENTS: i64 = 300;

pub(crate) const COPY_MAX_FILES: usize = 5_000;

pub(crate) const COPY_ENTRY_BUDGET: usize = 300_000;

pub(crate) const COPY_SIMILARITY: usize = 60;

pub(crate) const ARTIFACT_EXTENSIONS: [&str; 34] = [
    "tgz", "gz", "zip", "tar", "xz", "bz2", "zst", "7z", "whl", "jar", "war", "aar", "apk", "aab",
    "ipa", "dmg", "pkg", "deb", "rpm", "msi", "exe", "dll", "so", "dylib", "a", "o", "lib", "wasm",
    "node", "bin", "img", "iso", "sig", "map",
];

/// A file that build or release tooling produces: archives, packages, binaries, libraries and
/// checksums. A copy made of these is cheap to make again; a copy of photos or documents is not.
pub(crate) fn build_artifact(name: &[u8]) -> bool {
    let name = String::from_utf8_lossy(name).to_ascii_lowercase();
    match name.rsplit_once('.') {
        None => true,
        Some((_, ext)) => ARTIFACT_EXTENSIONS.contains(&ext),
    }
}

/// A relative path with every version replaced by `#`: a run of digits and dots, with any
/// prerelease suffix such as `-beta.2` or `-rc1`, so `app-1.2.0-beta.1.tgz` and `app-1.3.0.tgz`
/// name the same file.
pub(crate) fn fold_digits(path: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(path.len());
    let mut i = 0;
    while i < path.len() {
        if !path[i].is_ascii_digit() {
            out.push(path[i]);
            i += 1;
            continue;
        }
        let digits = |mut j: usize| {
            while j < path.len() && (path[j].is_ascii_digit() || path[j] == b'.') {
                j += 1;
            }
            j
        };
        i = digits(i);
        while path[..i].ends_with(b".") {
            i -= 1;
        }
        if i < path.len() && matches!(path[i], b'-' | b'.') {
            let rest = &path[i + 1..];
            if let Some(tag) = ["alpha", "beta", "rc", "pre", "dev", "next", "r"]
                .iter()
                .find(|t| {
                    rest.len() >= t.len() && rest[..t.len()].eq_ignore_ascii_case(t.as_bytes())
                })
            {
                let mut j = i + 1 + tag.len();
                if j < path.len() && path[j] == b'.' {
                    j += 1;
                }
                if j < path.len() && path[j].is_ascii_digit() {
                    i = digits(j);
                    while path[..i].ends_with(b".") {
                        i -= 1;
                    }
                }
            }
        }
        out.push(b'#');
    }
    out
}

/// At least `COPY_SIMILARITY` percent of the two folders' files are the same.
pub(crate) fn similar(a: &BTreeSet<Vec<u8>>, b: &BTreeSet<Vec<u8>>) -> bool {
    let shared = a.intersection(b).count();
    let all = a.len() + b.len() - shared;
    all > 0 && shared * 100 >= all * COPY_SIMILARITY
}

impl Index {
    /// Installers, disk images and archives in the Downloads folder, untouched for a month: what
    /// they installed or unpacked lives elsewhere, and they can be downloaded again.
    pub(crate) fn old_installers(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<Vec<Vec<u8>>> {
        let Some(downloads) =
            crate::rollup::relative_raw(root, &home.join("Downloads")).filter(|d| !d.is_empty())
        else {
            return Ok(vec![]);
        };
        let cutoff = (now() - INSTALLER_DAYS * 86_400) as i128 * 1_000_000_000;
        let mut upper = downloads.clone();
        upper.push(b'0');
        let mut below = downloads.clone();
        below.push(b'/');
        let mut statement = self.db.prepare_cached(
            "SELECT CAST(parent||name AS BLOB) FROM entries WHERE scan_id=?1 AND parent=?4 AND kind='file' AND excluded IS NULL AND allocated>=?2 AND modified_ns<?3 UNION ALL SELECT CAST(parent||name AS BLOB) FROM entries WHERE scan_id=?1 AND parent>=?5 AND parent<?6 AND kind='file' AND excluded IS NULL AND allocated>=?2 AND modified_ns<?3",
        )?;
        let rows = statement.query_map(
            params![
                scan,
                INSTALLER_MIN_BYTES,
                cutoff as i64,
                downloads,
                below,
                upper
            ],
            |r| r.get::<_, Vec<u8>>(0),
        )?;
        let mut out = Vec::new();
        for path in rows {
            let path = path?;
            let name =
                String::from_utf8_lossy(crate::index::split_path(&path).1).to_ascii_lowercase();
            if INSTALLER_EXTENSIONS.iter().any(|ext| {
                name.strip_suffix(ext)
                    .is_some_and(|stem| stem.ends_with('.') && stem.len() > 2)
            }) {
                out.push(path);
            }
        }
        Ok(out)
    }
    /// Older copies of a folder: sibling folders holding the same files (names compared with their
    /// digits folded, so `app-1.2.tgz` and `app-1.3.tgz` count as the same file), mostly build or
    /// release output, where a newer sibling exists. Every copy but the newest is listed for
    /// review; nothing is suggested. Metadata only, and bounded: a few hundred parents, a few
    /// thousand files per folder.
    pub(crate) fn superseded_copies(
        &self,
        scan: i64,
        root: &std::path::Path,
        home: &std::path::Path,
        claimed: &[Vec<u8>],
    ) -> Result<Vec<Vec<u8>>> {
        let Some(home_raw) = crate::rollup::relative_raw(root, home) else {
            return Ok(vec![]);
        };
        let mut groups = self.db.prepare_cached(
            "SELECT parent FROM dir_totals WHERE scan_id=? AND allocated>=? AND parent IS NOT NULL GROUP BY parent HAVING count(*)>=2 ORDER BY sum(allocated) DESC LIMIT ?",
        )?;
        let parents: Vec<Vec<u8>> = groups
            .query_map(params![scan, COPY_MIN_BYTES, COPY_PARENTS], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut members = self.db.prepare_cached(
            "SELECT path,coalesce(newest,0) FROM dir_totals WHERE scan_id=? AND parent=? AND allocated>=? ORDER BY allocated DESC LIMIT 12",
        )?;
        let mut files = self.db.prepare_cached(
            "SELECT CAST(parent||name AS BLOB),apparent FROM entries WHERE scan_id=?1 AND parent=?2 AND kind='file' AND excluded IS NULL UNION ALL SELECT CAST(parent||name AS BLOB),apparent FROM entries WHERE scan_id=?1 AND parent>=?3 AND parent<?4 AND kind='file' AND excluded IS NULL LIMIT ?5",
        )?;
        let mut budget = COPY_ENTRY_BUDGET;
        let mut out = Vec::new();
        for parent in parents {
            let below_home = policy::inside(&parent, &home_raw, true);
            let relative = parent.strip_prefix(home_raw.as_slice()).unwrap_or(&parent);
            // Only the user's own folders: tool areas, application data and dependency trees keep
            // versions side by side on purpose, and their own kinds judge them.
            let tool_area = relative.split(|b| *b == b'/').any(|part| {
                part.first() == Some(&b'.')
                    || part.eq_ignore_ascii_case(b"Library")
                    || part.eq_ignore_ascii_case(b"AppData")
                    || policy::REGENERABLE.iter().any(|r| r.as_bytes() == part)
            });
            if !below_home || tool_area || claimed.iter().any(|c| policy::inside(&parent, c, true))
            {
                continue;
            }
            let mut copies: Vec<FolderCopy> = Vec::new();
            let rows: Vec<(Vec<u8>, i64)> = members
                .query_map(params![scan, parent, COPY_MIN_BYTES], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            for (dir, newest) in rows {
                if budget == 0 {
                    return Ok(out);
                }
                let mut git = dir.clone();
                git.extend_from_slice(b"/.git");
                if self.entry_at(scan, &git)?.is_some()
                    || claimed.iter().any(|c| policy::inside(c, &dir, true))
                {
                    continue;
                }
                let mut below = dir.clone();
                below.push(b'/');
                let mut upper = dir.clone();
                upper.push(b'0');
                let listed: Vec<(Vec<u8>, i64)> = files
                    .query_map(
                        params![scan, dir, below, upper, COPY_MAX_FILES as i64 + 1],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )?
                    .collect::<rusqlite::Result<_>>()?;
                budget = budget.saturating_sub(listed.len());
                if listed.len() > COPY_MAX_FILES || listed.len() < 3 {
                    continue;
                }
                let mut names = BTreeSet::new();
                let (mut bytes, mut artifact_bytes) = (0u64, 0u64);
                for (path, apparent) in listed {
                    let rest = &path[dir.len()..];
                    if rest
                        .split(|b| *b == b'/')
                        .any(|part| policy::REGENERABLE.iter().any(|r| r.as_bytes() == part))
                    {
                        continue;
                    }
                    let size = apparent.max(0) as u64;
                    bytes += size;
                    let name = crate::index::split_path(&path).1;
                    if build_artifact(name.strip_prefix(b"/").unwrap_or(name)) {
                        artifact_bytes += size;
                    }
                    names.insert(fold_digits(rest));
                }
                if bytes >= COPY_MIN_BYTES as u64 && artifact_bytes * 2 >= bytes {
                    copies.push((dir, newest, names, bytes));
                }
            }
            copies.sort_by_key(|c| std::cmp::Reverse(c.1));
            for (i, candidate) in copies.iter().enumerate().skip(1) {
                let superseded = copies[..i].iter().any(|keeper| {
                    keeper.1 > candidate.1
                        && similar(&keeper.2, &candidate.2)
                        && keeper.3 * 2 >= candidate.3
                        && candidate.3 * 2 >= keeper.3
                });
                if superseded {
                    out.push(candidate.0.clone());
                }
            }
        }
        Ok(out)
    }
}
