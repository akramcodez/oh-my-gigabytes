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
| Release binary | `darwin-arm64` (built on macos-15) and `darwin-x64` (macos-15-intel) with `MACOSX_DEPLOYMENT_TARGET=15.0`; the signed packages installed and ran on macos-15, macos-15-intel and macos-26 (smoke run 39196125718). **Update 2026-10-24:** the Intel build was dropped from the release; only `darwin-arm64` ships. The Intel runner still runs the Rust tests in `Checks`. |

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
| `omg scan --root $HOME` (60 s cap) | Partial: 68,319 entries, 12,773 excluded, **129 unreadable directories** in 617 ms; not published without `--accept-gaps`. Cause (from the `issues` table and `ls` in the same session, which fails with "Operation not permitted"): macOS privacy protection. 68 are under `Library/Group Containers`, 10 under `Library/Containers`, the rest are Desktop, Documents, Downloads, Mail, Messages, Safari, the Photos library and similar |

**Finding: quarantined downloads are blocked on a real Mac.** `a_quarantined_copy_of_the_binary_runs_from_a_terminal` failed here. A copy of the ad-hoc signed `omg` with `com.apple.quarantine` set is killed (SIGKILL, exit 137, no output) and Gatekeeper shows "Apple could not verify it is free of malware". A direct run had worked once in an experiment where the same file had been run before it was quarantined, so the block depends on whether the file was seen before. Hosted runners do not enforce this. Consequences:
- A binary downloaded in a browser will not run until the user approves it in System Settings, because the beta is not notarized (decision of 2026-10-18). Installing through npm or `curl` does not set the quarantine attribute and is not affected; that path was not tested on this Mac.
- The test now runs only when `CI` is set, so it does not pop a dialog on a developer's Mac. On a real Mac the claim it checks is false.

### Release-readiness pass on the same Mac (2026-11-01)

Release binary (`cargo build --release -p omg-cli --no-default-features --locked`) unless noted.

| Check | Result |
| --- | --- |
| Unreadable folders in a published partial scan | **Defect found and fixed.** Before: `omg tree` and the Browse view showed Desktop, Documents, Downloads (and any mode-000 folder) as `0.00 B, 0 files`, the same as an empty folder. That breaks "unknown is not zero", and every default home scan on a Mac hits it. Now `Index::children` marks a folder `incomplete` when an issue was recorded at or below it (all folders when the 1,000 stored issues overflow), and `excluded` when it is a mandatory exclusion that is never opened (`.ssh`, `.Trash`, `.git`, iCloud folders, which also showed `0.00 B`). `omg tree` adds `"incomplete"` and `"excluded"`; sizes read `unknown`, `≥ N` or `not scanned`, and the Browse view adds `unreadable` or `N files · incomplete`. Checked on the real home index: Desktop, Documents, Downloads `unknown · unreadable`; Library `≥ 2.65 GiB`; `.ssh` and `.Trash` `not scanned`. Tests: `unreadable_folders_are_marked_incomplete_up_the_tree` (core, real mode-000 folder and an excluded `.git`) and `unreadable_folders_show_unknown_not_zero` (TUI) |
| Folder headers | The open folder's own total (Browse header, `omg tree` top level) now shows `≥` and `"incomplete": true` too. The flag on `Totals` is not serialized, because finding ids hash those totals and must not change |
| Dry-run preview | **Defect found and fixed.** Every preview on macOS said "Stale, inaccessible or redirected target" for targets that exist, because the native re-check (`planning::revalidate`) exists only on Linux and fails closed elsewhere. The preview now says "Preview cannot re-check targets on this platform yet". Implementing a macOS re-check is left for later |
| Interface walkthrough (pty, home index) | Open folder and back, Findings with tier filter, details, preview, add to OMG Trash and remove (CLI `omg trash` agrees, nothing on disk changed), options menu (no delete entry), scan with `r` (partial with the macOS hint), 200×40 layout, and "Terminal too small" at 40×12, 24×8 and 10×5. Fixed: the `protected` tier label ran into the size column |
| Supply chain | `cargo deny check` (cargo-deny 0.20.2): advisories, bans, licenses and sources ok. `node scripts/sbom.js` validates against CycloneDX 1.5 (specification 1.7.2 schemas, jsonschema 4.25.1) |
| Synthetic benchmark | `cargo run --release -p omg-core --example benchmark`: 10,100 entries, cold index 31 ms, warm reconciliation 29 ms, warm startup query 2 ms |
| `omg findings` paths | Paths were printed only as arrays of byte values. Each finding now also has a readable `path`; `entry.path` stays the raw bytes. On the home index the findings were sensible for macOS: large files for review (wallpaper videos, toolchains), the Cargo registry, npm cache and `Library/Caches/pip` as info, and a file inside an Apple container `protected` as personal data |
| Explaining partial scans | On macOS the CLI and TUI now say that privacy protection can block Desktop, Documents and Downloads and where to allow the terminal app. Granting Full Disk Access was not tried here |
| Delete keys on a platform without delete | The footer, menus and `?` help offered `d delete…` on macOS. Pressing it was refused safely ("not available in this build"), but in a default build on macOS the TUI checked only the build flag. Keys and menu entries are now hidden unless `capability::platform_permanent_delete()` allows delete; `cleanup.rs` still refuses on its own |
| Edge-case tree (mode-000 folder, symlink loop, link to `..`, FIFO, socket, 300 nested levels, NFD name, 255-byte name, tab/newline name, 50 MB file plus an APFS clone and a hard link, 1 GiB sparse file, 20,000-file folder) | 20,317 entries, equal to an independent walk path for path; totals equal to `du -sk` (181,940,224 bytes); hard link counted once; sparse file 4 KiB allocated; links and special files recorded and not followed; the APFS clone counted twice, as documented ("shared/cloned blocks unknown") |
| `/Applications` | 224,181 entries, 0 errors, 1.3 s |
| Concurrency and cancellation | A second scan gets "another omg process owns this operation"; Ctrl-C gives `cancelled` and keeps the previous published index |
| Service commands | `start`, `stop`, `service status` refuse with "not supported on this platform" |
| State directory | Mode 0755 refused; a new one is created 0700; a state path under `/tmp` now names `/private/tmp` in the error |
| Release assembly on a Mac | **Defect found and fixed.** `scripts/assemble-release.js` made its temporary state folder under `os.tmpdir()` (`/var/folders/…`, a link), which omg refuses, so assembly failed with "Cannot run the binary". It now uses the real path. With a throwaway key: manifest `analysis: development-macos`, `cleanup: disabled`, `service: disabled` |
| npm install cycle (as `smoke.yml`, prefix with a space) | Wrapper plus `darwin-arm64` package: install, `--version`, `doctor`, scan, status, reinstall, uninstall (state kept); offline bundle installs alone. One changed byte in either installed binary gives "Binary integrity verification failed". npm leaves no quarantine mark on the installed binary |

## Final check (2026-11-02), in progress

The pass in [mac-final-check.md](mac-final-check.md), on commit `527f4ef` (latest `main`; `Checks` run 41398549668 green on all nine platform legs and the supply-chain job). Environment: Mac17,9 (Apple M5 Pro), macOS 26.5.1 (25F80), arm64, Xcode Command Line Tools at `/Library/Developer/CommandLineTools`, Rust 1.95.0, Node 22.23.4, Python 3.9.6 (system). Run from Claude Code's shell in the owner's terminal, without Full Disk Access. Steps 0–3 done; steps 4–10 (Full Disk Access, iCloud, external drive, interface in several terminals, the CI-built package) still to do by hand.

| Step | Result |
| --- | --- |
| 0. Prepare | Pass. No `omg-*` disk image mounted before the run |
| 1. `scripts/mac-check.sh --smoke` | Pass: every summary line `PASS`. Tests 73 passed, 0 failed, 1 ignored in the default build and the same in the analysis-only build; PTY tests 7 run, OK, 5 skipped (the 2 delete flows, since macOS has no delete, and the 3 Windows ConPTY tests); `release.test.js` 21 tests, 18 passed, 0 failed, 3 skipped (two need an x86_64 Linux host, one is the offline npm install test). Smoke scan of `$HOME` (60 s cap): partial, 151,370 entries, 14,240 excluded, 131 unreadable directories, 4,548 ms; not published, with the privacy-protection hint. No `omg-*` image left mounted afterwards |
| 2. Release binary | `cargo build --release -p omg-cli --no-default-features --locked`; `--version` prints `omg 0.1.0-beta.1`; `doctor` prints `"cleanup": "unsupported"` and `permanent delete: unsupported (not included in this analysis-only build)`. The `mktemp -d` state folder is mode 0700 |
| 3. Refusals | `scan --root /` refused (sealed system volume); `--root /tmp` refused, naming `/private/tmp`; a mode-755 state folder refused ("must be owned by current user with private (0700) permissions"); `start` refused ("background service is not supported on this platform"). All exit 1 |

## Gates not passed

No macOS older than 15, only one real Mac (Apple silicon, macOS 26), no Intel Mac, no Full Disk Access / iCloud / Time Machine behaviour, no Apple notarization or code-signing identity, and the signed packages were assembled here only with a throwaway key (no production key exists). The macOS items in `docs/checklist.md` stay unchecked.
