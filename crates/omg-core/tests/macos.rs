//! macOS adapter tests (release plan R06). Disk images (`hdiutil`) give real APFS, HFS+, exFAT and
//! FAT volumes without a second disk. They are skipped with a message where `hdiutil` cannot attach
//! an image, unless `OMG_REQUIRE_DISK_IMAGES=1` (set in CI), which turns a skip into a failure.
#![cfg(target_os = "macos")]
use omg_core::{Index, ScanOptions, capability, platform, scanner};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    },
    time::Duration,
};

fn canonical_temp() -> tempfile::TempDir {
    // /var is a link on macOS and omg refuses links in roots and state paths.
    let directory = tempfile::Builder::new()
        .prefix("omg-macos-")
        .tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap())
        .unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

fn scan(root: &Path, state: &Path) -> (Index, omg_core::ScanReport) {
    let mut index = Index::open(state).unwrap();
    let report = scanner::scan(
        &mut index,
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
    (index, report)
}

#[test]
fn macos_offers_analysis_but_never_deletion() {
    assert_eq!(
        capability::platform_permanent_delete(),
        capability::Support::Unsupported
    );
    for family in [
        capability::Filesystem::Apfs,
        capability::Filesystem::Hfs,
        capability::Filesystem::Fat,
    ] {
        assert_eq!(
            capability::permanent_delete(family),
            capability::Support::Unsupported
        );
    }
    assert_eq!(
        capability::Filesystem::from_name("apfs"),
        capability::Filesystem::Apfs
    );
    assert_eq!(
        capability::Filesystem::from_name("hfs"),
        capability::Filesystem::Hfs
    );
    assert_eq!(
        capability::Filesystem::from_name("smbfs"),
        capability::Filesystem::Other
    );
}

#[test]
fn roots_through_links_the_system_volume_and_missing_paths_are_refused() {
    let base = canonical_temp();
    let real = base.path().join("real");
    fs::create_dir(&real).unwrap();
    assert!(
        platform::native::open_root(&real).is_ok(),
        "a real folder opens"
    );

    let link = base.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let error = platform::native::open_root(&link).unwrap_err().to_string();
    assert!(error.contains("symbolic link"), "{error}");

    // `/tmp` is a link to /private/tmp; the message tells the user what to do.
    assert!(
        platform::native::open_root(Path::new("/tmp"))
            .unwrap_err()
            .to_string()
            .contains("/private/tmp")
    );
    // The sealed, read-only system volume is never scanned.
    let error = platform::native::open_root(Path::new("/"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("system volume"), "{error}");
    assert!(platform::native::open_root(&base.path().join("absent")).is_err());
}

#[test]
fn scanning_a_home_like_tree_reports_apfs_totals_and_keeps_links_out() {
    let root = canonical_temp();
    let state = canonical_temp();
    fs::create_dir(root.path().join("docs")).unwrap();
    fs::write(root.path().join("docs/a.txt"), b"hello").unwrap();
    fs::write(root.path().join("docs/big.bin"), vec![7u8; 100 * 1024]).unwrap();
    fs::hard_link(
        root.path().join("docs/big.bin"),
        root.path().join("docs/big-link.bin"),
    )
    .unwrap();
    std::os::unix::fs::symlink("a.txt", root.path().join("docs/link")).unwrap();
    let sparse = fs::File::create(root.path().join("sparse")).unwrap();
    sparse.set_len(8 * 1024 * 1024).unwrap();

    let (index, report) = scan(root.path(), state.path());
    assert_eq!(report.state, "complete");
    assert_eq!(report.errors, 0);
    let entries = index.entries(1000, 0).unwrap();
    let find = |name: &[u8]| entries.iter().find(|e| e.path == name).unwrap();
    assert_eq!(find(b"docs/a.txt").apparent, 5);
    assert_eq!(
        find(b"docs/big.bin").identity,
        find(b"docs/big-link.bin").identity,
        "a hard link shares its identity"
    );
    assert_eq!(find(b"docs/big.bin").links, 2);
    assert_eq!(find(b"docs/link").kind, "symlink");
    assert!(
        find(b"docs/link").excluded.is_some(),
        "a link is recorded and never followed"
    );
    let sparse = find(b"sparse");
    assert!(
        sparse.apparent > sparse.allocated,
        "sparse apparent {} vs allocated {}",
        sparse.apparent,
        sparse.allocated
    );
    // The hard-linked data is counted once in the unique total.
    let unique: u64 = {
        let mut seen = std::collections::BTreeSet::new();
        entries
            .iter()
            .filter(|e| e.excluded.is_none() && seen.insert(e.identity.clone()))
            .map(|e| e.allocated)
            .sum()
    };
    assert_eq!(index.status().unwrap().unique_allocated, unique);
}

struct Image {
    mount: PathBuf,
}
impl Drop for Image {
    fn drop(&mut self) {
        for flags in [&[][..], &["-force"][..]] {
            let done = Command::new("hdiutil")
                .arg("detach")
                .args(flags)
                .arg(&self.mount)
                .output()
                .is_ok_and(|o| o.status.success());
            if done {
                break;
            }
        }
    }
}

/// Create and attach a 100 MB image with `filesystem`; `None` if the host cannot do it.
fn attach(filesystem: &str, tag: &str, mount: &Path, scratch: &Path) -> Option<Image> {
    let image = scratch.join(format!("{tag}.dmg"));
    let created = (0..3).any(|attempt| {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(3)); // "Resource busy" right after a detach
        }
        Command::new("hdiutil")
            .args(["create", "-size", "100m", "-fs", filesystem, "-volname"])
            .arg(format!("OMG{tag}"))
            .arg(&image)
            .output()
            .is_ok_and(|o| o.status.success())
    });
    if !created {
        return None;
    }
    fs::create_dir_all(mount).ok()?;
    Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-mountpoint"])
        .arg(mount)
        .arg(&image)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(Image {
        mount: mount.to_path_buf(),
    })
}

fn images_required() -> bool {
    std::env::var("OMG_REQUIRE_DISK_IMAGES").is_ok_and(|v| v == "1")
}

#[test]
fn every_macos_filesystem_scans_and_a_mounted_volume_is_a_boundary() {
    let scratch = canonical_temp();
    let state = canonical_temp();
    // (hdiutil -fs name, tag, supports hard links and symlinks)
    let matrix = [
        ("APFS", "apfs", true),
        ("Case-sensitive APFS", "csapfs", true),
        ("HFS+", "hfs", true),
        ("Case-sensitive HFS+", "cshfs", true),
        ("ExFAT", "exfat", false),
        ("MS-DOS FAT32", "fat32", false),
    ];
    let mut tested = Vec::new();
    for (filesystem, tag, links) in matrix {
        let mount = scratch.path().join(format!("mnt-{tag}"));
        let Some(_image) = attach(filesystem, tag, &mount, scratch.path()) else {
            assert!(
                !images_required(),
                "hdiutil could not attach a {filesystem} image"
            );
            eprintln!("skipping {filesystem}: hdiutil could not attach an image here");
            continue;
        };
        fs::create_dir(mount.join("src")).unwrap();
        fs::write(mount.join("src/main.rs"), b"fn main() {}\n").unwrap();
        fs::write(mount.join("data.bin"), vec![1u8; 50_000]).unwrap();
        if links {
            fs::hard_link(mount.join("data.bin"), mount.join("data-link.bin")).unwrap();
            std::os::unix::fs::symlink("data.bin", mount.join("ln")).unwrap();
        }

        let state_dir = state.path().join(tag);
        fs::create_dir(&state_dir).unwrap();
        fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let (index, report) = scan(&mount, &state_dir);
        assert_eq!(report.state, "complete", "{filesystem}");
        assert_eq!(report.errors, 0, "{filesystem}");
        let entries = index.entries(1000, 0).unwrap();
        let find = |name: &[u8]| entries.iter().find(|e| e.path == name);
        assert_eq!(find(b"src/main.rs").unwrap().apparent, 13, "{filesystem}");
        assert_eq!(find(b"data.bin").unwrap().apparent, 50_000, "{filesystem}");
        if links {
            assert_eq!(
                find(b"data.bin").unwrap().identity,
                find(b"data-link.bin").unwrap().identity,
                "{filesystem} hard link identity"
            );
            assert!(find(b"ln").unwrap().excluded.is_some(), "{filesystem}");
        }
        tested.push(tag);
    }
    if images_required() {
        assert_eq!(tested.len(), 6, "all disk images ran: {tested:?}");
    }

    // A volume mounted inside the scan root is a boundary: recorded, never entered.
    let outer = canonical_temp();
    let outer_state = canonical_temp();
    let inside = outer.path().join("vol");
    let Some(_image) = attach("APFS", "inner", &inside, scratch.path()) else {
        assert!(
            !images_required(),
            "hdiutil could not attach the inner image"
        );
        return;
    };
    fs::write(inside.join("secret-looking-name"), b"x").unwrap();
    fs::write(outer.path().join("top.txt"), b"top").unwrap();
    let (index, report) = scan(outer.path(), outer_state.path());
    assert_eq!(report.state, "complete");
    let entries = index.entries(1000, 0).unwrap();
    let volume = entries.iter().find(|e| e.path == b"vol").unwrap();
    assert_eq!(
        volume.excluded.as_deref(),
        Some("mount boundary not crossed")
    );
    assert!(
        !entries
            .iter()
            .any(|e| e.path.starts_with(b"vol/") || e.path == b"vol/secret-looking-name"),
        "nothing below a mounted volume is indexed"
    );
    assert!(entries.iter().any(|e| e.path == b"top.txt"));
}
