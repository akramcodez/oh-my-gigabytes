//! Native Windows scanning and deletion (docs/windows-implementation-plan.md, W01, W02, W08).
//! Runs only on Windows.
#![cfg(windows)]
use omg_core::{Index, ScanOptions, model::Entry, platform, scanner};
use std::{
    ffi::OsString,
    fs,
    os::windows::ffi::OsStringExt,
    path::Path,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    },
};

fn scan(index: &mut Index, root: &Path) {
    let report = scanner::scan(
        index,
        &ScanOptions {
            root: root.to_path_buf(),
            workers: 2,
            exclusions: vec![],
            full: true,
            publish_partial: false,
        },
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicU64::new(0)),
    )
    .unwrap();
    assert_eq!(report.state, "complete", "{report:?}");
}

fn all_entries(index: &Index) -> Vec<Entry> {
    index.entries(100_000, 0).unwrap()
}

fn entry_named<'a>(entries: &'a [Entry], relative: &str) -> Option<&'a Entry> {
    let wanted = relative.replace('\\', "/");
    entries.iter().find(|e| e.path == wanted.as_bytes())
}

/// Recursive counts and apparent bytes from `std::fs`, an oracle independent of the scanner.
fn oracle(root: &Path) -> (u64, u64, u64) {
    let (mut files, mut dirs, mut bytes) = (0, 0, 0);
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for item in fs::read_dir(&dir).unwrap() {
            let item = item.unwrap();
            let meta = fs::symlink_metadata(item.path()).unwrap();
            if meta.is_dir() && !meta.file_type().is_symlink() {
                dirs += 1;
                stack.push(item.path());
            } else if meta.is_file() {
                files += 1;
                bytes += meta.len();
            }
        }
    }
    (files, dirs, bytes)
}

/// A state directory only you can use, as `Index::open` requires (the Unix tests use mode 0700).
fn state() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    platform::windows_state::protect(dir.path()).unwrap();
    dir
}

#[test]
fn totals_match_an_independent_walk() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path();
    fs::create_dir_all(root.join("alpha/beta")).unwrap();
    fs::create_dir(root.join("gamma")).unwrap();
    fs::write(root.join("alpha/one.bin"), vec![1u8; 5000]).unwrap();
    fs::write(root.join("alpha/beta/two.txt"), b"hello").unwrap();
    fs::write(root.join("gamma/three.dat"), vec![2u8; 123]).unwrap();
    let state = state();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, root);

    let (files, dirs, bytes) = oracle(root);
    let entries = all_entries(&index);
    let scanned_files = entries.iter().filter(|e| e.kind == "file").count() as u64;
    let scanned_dirs = entries.iter().filter(|e| e.kind == "directory").count() as u64;
    assert_eq!((scanned_files, scanned_dirs), (files, dirs));
    let apparent: u64 = entries
        .iter()
        .filter(|e| e.kind == "file")
        .map(|e| e.apparent)
        .sum();
    assert_eq!(apparent, bytes);
    let status = index.status().unwrap();
    assert_eq!(status.unique_apparent, bytes);
    assert!(entry_named(&entries, "alpha/beta/two.txt").is_some());
}

#[test]
fn junctions_are_recorded_but_never_followed() {
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), b"not part of the scan").unwrap();
    let home = tempfile::tempdir().unwrap();
    let root = home.path();
    fs::write(root.join("kept.txt"), b"kept").unwrap();
    let link = root.join("link");
    // mklink /J needs no administrator rights, unlike symbolic links.
    let made = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(outside.path())
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );

    let state = state();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, root);
    let entries = all_entries(&index);
    let junction = entry_named(&entries, "link").expect("junction is listed");
    assert_eq!(junction.kind, "symlink");
    assert_eq!(
        junction.excluded.as_deref(),
        Some("symlink/special entry not followed")
    );
    assert!(
        entries.iter().all(|e| !e.path.starts_with(&b"link/"[..])),
        "nothing below the junction is indexed"
    );
    assert!(entry_named(&entries, "kept.txt").is_some());
}

#[test]
fn hard_links_share_one_identity() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path();
    fs::write(root.join("original.bin"), vec![9u8; 4096]).unwrap();
    fs::hard_link(root.join("original.bin"), root.join("alias.bin")).unwrap();
    let state = state();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, root);
    let entries = all_entries(&index);
    let original = entry_named(&entries, "original.bin").unwrap();
    let alias = entry_named(&entries, "alias.bin").unwrap();
    assert_eq!(original.identity, alias.identity);
}

#[test]
fn paths_longer_than_max_path_are_scanned() {
    let home = tempfile::tempdir().unwrap();
    let mut deep = home.path().to_path_buf();
    for _ in 0..12 {
        deep.push("a-directory-with-a-fairly-long-name");
    }
    // Verbatim form lifts the MAX_PATH limit for creation.
    let verbatim = Path::new(r"\\?\").join(&deep);
    fs::create_dir_all(&verbatim).unwrap();
    fs::write(verbatim.join("deep.txt"), b"deep").unwrap();
    assert!(deep.join("deep.txt").as_os_str().len() > 260);

    let state = state();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, home.path());
    let entries = all_entries(&index);
    assert!(entries.iter().any(|e| e.path.ends_with(&b"/deep.txt"[..])));
}

#[test]
fn unicode_and_unpaired_surrogate_names_round_trip() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path();
    fs::write(root.join("café 😀 報告.txt"), b"unicode").unwrap();
    // An unpaired surrogate is legal in an NTFS name but not in UTF-8; WTF-8 keeps it.
    let lone = OsString::from_wide(&[0x0062, 0xD800, 0x0063]);
    if fs::write(root.join(&lone), b"lone").is_err() {
        eprintln!("skipped: this volume refused an unpaired surrogate name");
        return;
    }
    let state = state();
    let mut index = Index::open(state.path()).unwrap();
    scan(&mut index, root);
    let names: Vec<OsString> = all_entries(&index)
        .iter()
        .map(|e| platform::path(&e.path).unwrap().into_os_string())
        .collect();
    assert!(names.contains(&OsString::from("café 😀 報告.txt")));
    assert!(names.contains(&lone));
}

#[test]
fn a_link_as_the_scan_root_is_refused() {
    let outside = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let link = home.path().join("root-link");
    let made = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(outside.path())
        .output()
        .unwrap();
    assert!(made.status.success());
    let state = state();
    let mut index = Index::open(state.path()).unwrap();
    let error = scanner::scan(
        &mut index,
        &ScanOptions {
            root: link,
            workers: 2,
            exclusions: vec![],
            full: true,
            publish_partial: false,
        },
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicU64::new(0)),
    )
    .unwrap_err();
    assert!(error.to_string().contains("link or mount point"), "{error}");
}

/// Permanent deletion on NTFS (docs/windows-implementation-plan.md, W08): the same safety cases as
/// the Linux suite in tests/cleanup.rs.
#[cfg(feature = "permanent-delete")]
mod deletion {
    use super::*;
    use omg_core::cleanup::DeleteReport;
    use std::os::windows::fs::OpenOptionsExt;

    /// A scratch folder with no Git repository above it. A home folder that is itself a repository
    /// (the owner's is) makes the repository rule refuse every plain folder below it, so such a run
    /// is skipped with a message instead of failing.
    fn sandbox() -> Option<tempfile::TempDir> {
        let dir = tempfile::tempdir().unwrap();
        if dir
            .path()
            .ancestors()
            .skip(1)
            .any(|ancestor| ancestor.join(".git").exists())
        {
            eprintln!(
                "skipped: {} is inside a Git repository; set TMP to a folder outside one",
                dir.path().display()
            );
            return None;
        }
        Some(dir)
    }

    fn id_of(index: &Index, path: &[u8]) -> i64 {
        index
            .marked()
            .unwrap()
            .into_iter()
            .find(|m| m.path == path)
            .unwrap()
            .id
    }

    fn delete(index: &mut Index, ids: &[i64], home: &Path) -> DeleteReport {
        let plan = index.prepare_delete(ids, home).unwrap();
        index
            .delete_permanently(&plan, &plan.fingerprint.clone(), home)
            .unwrap()
    }

    fn junction(link: &Path, target: &Path) {
        let made = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            made.status.success(),
            "{}",
            String::from_utf8_lossy(&made.stderr)
        );
    }

    #[test]
    fn confirmed_folder_and_file_are_deleted_including_read_only_contents() {
        let Some(dir) = sandbox() else { return };
        let h = dir.path();
        fs::create_dir_all(h.join("scratch/nested")).unwrap();
        fs::write(h.join("scratch/a.bin"), vec![1u8; 5000]).unwrap();
        fs::write(h.join("scratch/nested/b.txt"), b"b").unwrap();
        let read_only = h.join("scratch/nested/read-only.txt");
        fs::write(&read_only, b"ro").unwrap();
        let mut permissions = fs::metadata(&read_only).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&read_only, permissions).unwrap();
        fs::write(h.join("single.log"), b"log").unwrap();
        fs::write(h.join("keep.txt"), b"keep").unwrap();
        let state = state();
        let mut index = Index::open(state.path()).unwrap();
        scan(&mut index, h);
        index.mark(b"scratch", 4096, h).unwrap();
        index.mark(b"single.log", 3, h).unwrap();
        let ids = [id_of(&index, b"scratch"), id_of(&index, b"single.log")];
        let report = delete(&mut index, &ids, h);
        assert_eq!(report.deleted.len(), 2, "{report:?}");
        assert!(!h.join("scratch").exists());
        assert!(!h.join("single.log").exists());
        assert_eq!(fs::read(h.join("keep.txt")).unwrap(), b"keep");
        assert!(index.entry(b"scratch").unwrap().is_none());
        assert!(index.marked().unwrap().is_empty());
    }

    #[test]
    fn replaced_file_is_skipped_and_the_replacement_survives() {
        let Some(dir) = sandbox() else { return };
        let h = dir.path();
        fs::write(h.join("swap-me"), b"original").unwrap();
        let state = state();
        let mut index = Index::open(state.path()).unwrap();
        scan(&mut index, h);
        index.mark(b"swap-me", 8, h).unwrap();
        // Same name, new object. NTFS tunnelling may even restore the old creation time, so the
        // file id is what must catch this.
        fs::remove_file(h.join("swap-me")).unwrap();
        fs::write(h.join("swap-me"), b"new important data").unwrap();
        let id = id_of(&index, b"swap-me");
        let report = delete(&mut index, &[id], h);
        assert!(report.deleted.is_empty(), "{report:?}");
        assert!(report.skipped[0].1.contains("replaced"), "{report:?}");
        assert_eq!(fs::read(h.join("swap-me")).unwrap(), b"new important data");
    }

    #[test]
    fn junction_inside_is_removed_but_its_target_survives() {
        let Some(dir) = sandbox() else { return };
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), b"outside").unwrap();
        let h = dir.path();
        fs::create_dir(h.join("scratch")).unwrap();
        fs::write(h.join("scratch/a.bin"), b"data").unwrap();
        junction(&h.join("scratch").join("link"), outside.path());
        let state = state();
        let mut index = Index::open(state.path()).unwrap();
        scan(&mut index, h);
        index.mark(b"scratch", 4096, h).unwrap();
        let id = id_of(&index, b"scratch");
        let report = delete(&mut index, &[id], h);
        assert_eq!(report.deleted.len(), 1, "{report:?}");
        assert!(!h.join("scratch").exists());
        assert_eq!(
            fs::read(outside.path().join("secret.txt")).unwrap(),
            b"outside"
        );
    }

    #[test]
    fn folder_swapped_for_a_junction_never_leads_outside() {
        let Some(dir) = sandbox() else { return };
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), b"outside").unwrap();
        let h = dir.path().to_path_buf();
        fs::create_dir(h.join("scratch")).unwrap();
        fs::write(h.join("scratch/a.bin"), b"data").unwrap();
        let state = state();
        let mut index = Index::open(state.path()).unwrap();
        scan(&mut index, &h);
        index.mark(b"scratch", 4096, &h).unwrap();
        let plan = index
            .prepare_delete(&[id_of(&index, b"scratch")], &h)
            .unwrap();
        let report = index
            .delete_with_control(&plan, &plan.fingerprint, &h, None, |path| {
                if path == b"scratch" {
                    fs::remove_dir_all(h.join("scratch")).unwrap();
                    junction(&h.join("scratch"), outside.path());
                }
            })
            .unwrap();
        assert!(report.deleted.is_empty(), "{report:?}");
        assert!(report.skipped[0].1.contains("replaced"), "{report:?}");
        assert_eq!(
            fs::read(outside.path().join("secret.txt")).unwrap(),
            b"outside"
        );
    }

    #[test]
    fn protected_child_inserted_after_validation_stops_the_delete() {
        let Some(dir) = sandbox() else { return };
        let h = dir.path().to_path_buf();
        fs::create_dir(h.join("scratch")).unwrap();
        fs::write(h.join("scratch/a.bin"), b"data").unwrap();
        let state = state();
        let mut index = Index::open(state.path()).unwrap();
        scan(&mut index, &h);
        index.mark(b"scratch", 4096, &h).unwrap();
        let plan = index
            .prepare_delete(&[id_of(&index, b"scratch")], &h)
            .unwrap();
        let report = index
            .delete_with_control(&plan, &plan.fingerprint, &h, None, |path| {
                if path == b"scratch" {
                    fs::create_dir(h.join("scratch/.ssh")).unwrap();
                    fs::write(h.join("scratch/.ssh/id_ed25519"), b"key").unwrap();
                }
            })
            .unwrap();
        assert!(report.deleted.is_empty(), "{report:?}");
        assert!(report.skipped[0].1.contains("sensitive"), "{report:?}");
        assert_eq!(fs::read(h.join("scratch/.ssh/id_ed25519")).unwrap(), b"key");
        assert_eq!(fs::read(h.join("scratch/a.bin")).unwrap(), b"data");
    }

    #[test]
    fn file_held_open_without_delete_sharing_is_skipped() {
        let Some(dir) = sandbox() else { return };
        let h = dir.path();
        fs::write(h.join("busy.bin"), b"open elsewhere").unwrap();
        let state = state();
        let mut index = Index::open(state.path()).unwrap();
        scan(&mut index, h);
        index.mark(b"busy.bin", 14, h).unwrap();
        // Another program holds it and allows reading only (no delete sharing).
        let holder = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(h.join("busy.bin"))
            .unwrap();
        let id = id_of(&index, b"busy.bin");
        let report = delete(&mut index, &[id], h);
        drop(holder);
        assert!(report.deleted.is_empty(), "{report:?}");
        assert!(report.skipped[0].1.contains("in use"), "{report:?}");
        assert_eq!(fs::read(h.join("busy.bin")).unwrap(), b"open elsewhere");
    }

    #[test]
    fn removing_one_hard_link_name_keeps_the_data_and_flags_totals() {
        let Some(dir) = sandbox() else { return };
        let h = dir.path();
        fs::create_dir(h.join("scratch")).unwrap();
        fs::create_dir(h.join("keep")).unwrap();
        fs::write(h.join("keep/data.bin"), vec![7u8; 8192]).unwrap();
        fs::hard_link(h.join("keep/data.bin"), h.join("scratch/alias.bin")).unwrap();
        let state = state();
        let mut index = Index::open(state.path()).unwrap();
        scan(&mut index, h);
        index.mark(b"scratch", 8192, h).unwrap();
        let id = id_of(&index, b"scratch");
        let report = delete(&mut index, &[id], h);
        assert_eq!(report.deleted.len(), 1, "{report:?}");
        assert!(report.needs_reconcile, "{report:?}");
        assert_eq!(fs::read(h.join("keep/data.bin")).unwrap().len(), 8192);
    }
}

/// The owner-only state directory (the Windows counterpart of the Unix 0700 check).
mod state_acl {
    use super::*;
    use std::ffi::OsStr;

    fn icacls(args: &[&OsStr]) -> String {
        let run = Command::new("icacls").args(args).output().unwrap();
        assert!(
            run.status.success(),
            "icacls {args:?}: {}{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        String::from_utf8_lossy(&run.stdout).into_owned()
    }

    #[test]
    fn directories_omg_creates_are_closed_to_other_users() {
        let base = tempfile::tempdir().unwrap();
        let fresh = base.path().join("first").join("second");
        drop(Index::open(&fresh).unwrap());
        platform::windows_state::verify_private(&fresh).unwrap();
        platform::windows_state::verify_private(&base.path().join("first")).unwrap();
        let listing = icacls(&[fresh.as_os_str()]);
        for other in ["Everyone", "BUILTIN\\Users", "Authenticated Users"] {
            assert!(
                !listing.contains(other),
                "{other} must not be listed:\n{listing}"
            );
        }
        assert!(listing.contains("NT AUTHORITY\\SYSTEM"), "{listing}");
    }

    #[test]
    fn a_directory_open_to_everyone_is_refused_and_diagnosis_does_not_repair_it() {
        let dir = tempfile::tempdir().unwrap();
        platform::windows_state::protect(dir.path()).unwrap();
        drop(Index::open(dir.path()).unwrap());

        icacls(&[
            dir.path().as_os_str(),
            OsStr::new("/grant"),
            OsStr::new("Everyone:(OI)(CI)R"),
        ]);
        let error = Index::open(dir.path())
            .err()
            .expect("open to everyone")
            .to_string();
        assert!(error.contains("other users"), "{error}");
        assert!(
            error.contains("icacls"),
            "the message says how to fix it: {error}"
        );
        assert!(omg_core::index::diagnose(dir.path()).is_err());
        assert!(
            icacls(&[dir.path().as_os_str()]).contains("Everyone"),
            "refusing must not change the access list"
        );

        icacls(&[
            dir.path().as_os_str(),
            OsStr::new("/remove"),
            OsStr::new("Everyone"),
        ]);
        drop(Index::open(dir.path()).unwrap());
    }
}
