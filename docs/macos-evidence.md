# macOS evidence

Status: **native analysis-only adapter, tested only on GitHub-hosted runners.** `platform::macos` scans APFS, HFS+, exFAT and FAT volumes and never changes a file. Permanent delete, OS Trash and a background service do not exist on macOS. Hosted runners were the first test bed; one real Mac has now run the suite once (see "Real Mac" below). Nothing here is a certification. See [beta-release-progress.md](beta-release-progress.md).

## Test bed

| Label | macOS | Arch | Notes |
| --- | --- | --- | --- |
| `macos-26` | 26.6.2 (25G83) | arm64 | internal volume APFS, system volume sealed |
| `macos-15` | 15.7.9 (24G830) | arm64 | same layout |
| `macos-15-intel` | 15.7.9 (24G830) | x86_64 | same layout; GitHub retires Intel images in fall 2027 |

The repository is public, so standard hosted runners are free. Workflow: `.github/workflows/macos-probe.yml`; probe sources: `spikes/macos-probe/probe.c` and `images.sh`. First run: GitHub Actions run 39106110919 on commit `4f6e8f3`, all three jobs passed.

## Measured (R03, first run)

| Question | Result on all three runners |
| --- | --- |
| `O_NOFOLLOW_ANY` (0x20000000) | Present. Fails with `ELOOP` for a symlink anywhere in the path: mid-path, final, and a symlink leaving the root. Plain `O_NOFOLLOW` follows mid-path symlinks, so it is **not** enough. |
| `O_RESOLVE_BENEATH` (0x1000) | Present and enforced. An in-root symlink and an inner `..` are allowed. A symlink leaving the root, `../`, and an absolute path are refused. |
| Error for an escape attempt | macOS 26: `ENOTCAPABLE` (107). macOS 15: `EACCES` (13). The adapter must treat both as "denied". |
| SDK differences | `AT_RESOLVE_BENEATH` and `ENOTCAPABLE` are missing from the macOS 15 SDK headers but `O_RESOLVE_BENEATH` works at runtime. |
| `fstatat(AT_SYMLINK_NOFOLLOW)` | Reports `S_IFLNK` for a symlink. |
| Stat fields | `st_birthtimespec`, `st_flags`, `st_blocks`, `st_nlink`, `st_dev`, `st_ino` all available. A 10 MiB sparse file reported 0 blocks on APFS. |
| Flag constants | `UF_COMPRESSED` 0x20, `SF_DATALESS` 0x40000000, `SF_RESTRICTED` 0x80000, `UF_DATAVAULT` 0x80, `MNT_LOCAL` 0x1000, `MNT_DONTBROWSE` 0x100000, `MNT_ROOTFS` 0x4000. |
| `getattrlistbulk` | Works on a directory descriptor (returned all 4 entries in one call). Not needed for the first adapter. |
| Volumes | `/` is a sealed, read-only APFS system volume (`MNT_ROOTFS`, read-only). The home folder and temp folders live on the writable Data volume (`/System/Volumes/Data`, `MNT_DONTBROWSE`). |
| Boundaries | Mounted disk images get their own `st_dev`, so a device change marks a mount boundary. `/` and the Data volume report the **same** `st_dev`, so `st_dev` alone is not enough for system paths. |
| Disk images | APFS, case-sensitive APFS, HFS+, case-sensitive HFS+, ExFAT and FAT32 all create, mount, report `f_fstypename` (`apfs`, `hfs`, `exfat`, `msdos`) and detach with `hdiutil`. One FAT32 creation failed once with "Resource busy" on `macos-15`; a retry was added. |
| Scan roots by absolute path with `O_NOFOLLOW_ANY` (second run, commit `1d8c60c`, run 39106517191) | OK for `/Users`, `/Users/runner`, `/System/Volumes/Data`, `/private/tmp`, `/Volumes` and `/` (firmlinks are not links). `ELOOP` for `/tmp`, `/var/tmp`, `/var/folders/...` and any `hdiutil` mount path given as `/var/...`, because `/tmp` and `/var` are symlinks to `/private/...`. A scan root or state directory must therefore be given by its real path. Tests use a canonical `TMPDIR`. |
| FAT32 image retry | The retry added after the first run fixed the one-off "Resource busy" failure; all six image types mounted on all three runners. |
| Case sensitivity | Default APFS/HFS+ and ExFAT/FAT32 are case-insensitive; the case-sensitive images are not. Name matching for protection must not assume either. |
| Inode counts | ExFAT and FAT32 report `f_files` of 1 and 0, so the used-inode figure is unknown there (no progress estimate). |

## Decisions that follow

- Use `openat` with `O_NOFOLLOW_ANY | O_RESOLVE_BENEATH` for every child, which matches Linux's `RESOLVE_NO_SYMLINKS | RESOLVE_BENEATH`.
- Detect volume boundaries with `st_dev` and refuse system-volume roots through `MNT_ROOTFS`.
- Require a Darwin kernel new enough to enforce the flags. An older kernel could ignore an unknown open flag without error, which would silently drop the protection. Measured: macOS 15 and 26 only.
- Treat ExFAT and FAT32 inode counts as unknown.
- Match protected names case-insensitively on macOS (conservative: more is protected).

## Not verified (cannot be tested on hosted runners)

- Privacy protection prompts (TCC / Full Disk Access) when scanning Desktop, Documents, Downloads or Library.
- iCloud "dataless" placeholder files (no iCloud account on runners). Only the flag value is known.
- Time Machine local snapshots, a real internal APFS container with several volumes, external drives.
- Gatekeeper and quarantine behaviour of a binary a user downloads in a browser.
- Anything on macOS older than 15.

## Adapter results (2026-10-18 and 2026-10-19)

`platform::macos` follows the decisions above. Verified on `macos-15`, `macos-15-intel` and `macos-26` (Checks run 39111223427, commit `ad22c81`): fmt, clippy `-D warnings` (default and analysis-only builds), the full workspace tests, the PTY terminal smoke test and the Node release tests.

| Check | Result |
| --- | --- |
| Roots through a link (`/tmp`) | Refused with a hint to use `/private/tmp`; a real folder opens |
| Sealed system volume (`/`) | Refused (`MNT_ROOTFS`) |
| Home-like tree on APFS | Totals, hard-link identity (links = 2), links recorded and not followed, sparse apparent greater than allocated, unique total counts hard-linked data once |
| Disk-image matrix (`tests/macos.rs`) | APFS, case-sensitive APFS, HFS+, case-sensitive HFS+, exFAT and FAT32 images each mounted, scanned with no errors, sizes exact, hard links and links on the volumes that support them |
| Mount boundary | A mounted image inside the scan root is recorded as "mount boundary not crossed" and nothing below it is indexed |
| Delete capability | `omg doctor` says `permanent delete: unsupported`; APFS, HFS+ and FAT are refused by `capability::permanent_delete` |
| Quarantined copy of the binary | On hosted runners a copy with `com.apple.quarantine` set runs from a terminal (`--version`). **This does not hold on a real Mac:** see "Real Mac" below |
| Raw-byte file names | APFS rejects file names that are not valid UTF-8 (`EILSEQ`), so that fixture is created only where allowed |
| Release binary | `darwin-arm64` (built on macos-15) and `darwin-x64` (macos-15-intel) with `MACOSX_DEPLOYMENT_TARGET=15.0`; the signed packages installed and ran on macos-15, macos-15-intel and macos-26 (smoke run 39196125718) |

Minimum macOS is 15: the adapter checks `uname -r` for Darwin 24 or newer before opening a root.

## Real Mac (2026-11-01)

Environment: Mac17,9 (Apple M5 Pro), macOS 26.5.1 (25F80), arm64, internal volume APFS (sealed system volume), Xcode Command Line Tools (Apple clang 21.0.0), Rust 1.95.0 (from `rust-toolchain.toml`), Node 22.23.4, Python 3.9.6 (system). Run as a normal user; whether the terminal has Full Disk Access was not checked.

Command: `scripts/mac-check.sh --smoke` (mirrors the macOS legs of `.github/workflows/check.yml`; sets a canonical `TMPDIR` and `OMG_REQUIRE_DISK_IMAGES=1`, uses `--no-fail-fast` for the test runs).

| Check | Result |
| --- | --- |
| `cargo fmt --check`, clippy `-D warnings` (default and analysis-only) | Pass |
| `cargo test --workspace --locked` and `--no-default-features` (with `--no-fail-fast`) | Pass: 118 passed, 0 failed, 2 ignored, summed over both builds. `tests/macos.rs` mounted its disk images with `OMG_REQUIRE_DISK_IMAGES=1`, and no `omg-*` image was left mounted afterwards |
| PTY terminal smoke test (`tests/test_terminal.py`) | Pass on system Python 3.9.6 |
| `node --test packages/oh-my-gigabytes/test/release.test.js` | Pass |
| `omg doctor` | `permanent delete: unsupported`. The line still lists Linux filesystems in its hint text |
| Scan of a canonical temp tree (7 entries, link to `/etc` inside) | Complete, 0 errors, 1,049,005 unique apparent bytes |
| `omg scan --root /` and `--root /tmp` | Refused; `/tmp` prints the hint to use `/private/tmp` |
| `omg scan --root $HOME` (60 s cap) | Partial: 68,319 entries, 12,773 excluded, **129 unreadable or changed directories** in 617 ms; not published without `--accept-gaps`. The output does not say why; macOS privacy protection is a suspect but was not verified |

**Finding: quarantined downloads are blocked on a real Mac.** `a_quarantined_copy_of_the_binary_runs_from_a_terminal` failed here. A copy of the ad-hoc signed `omg` with `com.apple.quarantine` set is killed (SIGKILL, exit 137, no output) and Gatekeeper shows "Apple could not verify it is free of malware". A direct run had worked once in an experiment where the same file had been run before it was quarantined, so the block depends on whether the file was seen before. Hosted runners do not enforce this. Consequences:
- A binary downloaded in a browser will not run until the user approves it in System Settings, because the beta is not notarized (decision of 2026-10-18). Installing through npm or `curl` does not set the quarantine attribute and is not affected; that path was not tested on this Mac.
- The test now runs only when `CI` is set, so it does not pop a dialog on a developer's Mac. On a real Mac the claim it checks is false.

## Gates not passed

No macOS older than 15, only one real Mac (Apple silicon, macOS 26), no Intel Mac, no Full Disk Access / iCloud / Time Machine behaviour, the cause of the 129 home-folder errors is undetermined, no Apple notarization or code-signing identity, and no run of the signed npm package on this Mac. The macOS items in `docs/checklist.md` stay unchecked.
