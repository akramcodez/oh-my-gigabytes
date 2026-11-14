# macOS evidence

Status: **native analysis-only adapter, tested only on GitHub-hosted runners.** `platform::macos` scans APFS, HFS+, exFAT and FAT volumes and never changes a file. Permanent delete, OS Trash and a background service do not exist on macOS. Hosted runners were the first test bed; one real Mac has now run the suite once (see "Real Mac" below). Nothing here is a certification. See [release.md](release.md#not-passed-yet).

## Test bed

| Label | macOS | Arch | Notes |
| --- | --- | --- | --- |
| `macos-26` | 26.6.2 (25G83) | arm64 | internal volume APFS, system volume sealed |
| `macos-15` | 15.7.9 (24G830) | arm64 | same layout |
| `macos-15-intel` | 15.7.9 (24G830) | x86_64 | same layout; GitHub retires Intel images in fall 2027 |

The repository is public, so standard hosted runners are free. The probe workflow and its sources (`spikes/macos-probe`) were removed once the adapter existed; they are in Git history. First run: GitHub Actions run 39106110919 on commit `4f6e8f3`, all three jobs passed.

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

### Steps 4 and 9 (2026-11-05)

Same Mac (Mac17,9, Apple M5 Pro) but now macOS 27.2 (26B123, Darwin 27.2.0), Xcode at `/Applications/Xcode.app/Contents/Developer`, Rust 1.95.0, Node 22.23.4, `gh` 2.103.3. Run from Claude Code's shell in the owner's terminal. Step 4 used the release binary built on 2026-11-02 from `527f4ef`; the commits since (through `cb1e252`) change only documents. Step 4 and 9 numbers below come from this run only; none of this was run on 26.5.1 again.

| Step | Result |
| --- | --- |
| 4. Home folder without Full Disk Access | Pass, with one case not exercised. Strict scan of `$HOME`: partial, not published, 528,665 entries, 430 unreadable directories, 12,871 excluded, 4,830 ms, with the message naming Desktop, Documents and Downloads and where to allow the terminal. `--accept-gaps`: published as partial, 528,663 entries, 429 unreadable, 4,240 ms. `Library` shows `≥ 7.63 GiB` with `incomplete: true` (a lower bound, not `0.00 B`). `.ssh` and `.Trash` have no children and `protected: 1` (the 25-row default `tree` page did not list them, so a literal `"excluded": true` was not seen). Findings: 80 (67 `review` and 7 `protected` large files, 6 `info` package caches); none under Mail, Messages, Safari or containers. **Not exercised:** this terminal already had access to Desktop, Documents and Downloads, so they scanned normally (Desktop 14.20 MiB, Downloads 2.06 GiB, `incomplete: false`) and no macOS privacy question appeared; the "Don't Allow" case for those three folders is untested |
| 5. Full Disk Access | Not done (the setting has to be changed by hand) |
| 6. iCloud placeholder files | Not done: no `dataless` file found under Desktop or Documents |
| 7. Real external drive | Not done: no drive attached (only mounted disk images and Recovery under `/Volumes`) |
| 8. Interface in terminal apps | **Partly, and not in a real terminal app.** The release binary (built from `527f4ef`) was driven through a pseudo-terminal at 140×40 with the repository's `tests/terminal_screen.py` screen reader, on a throwaway `HOME` holding a few files and a mode-000 folder (22 of 22 checks passed): `1`–`4` switch Overview, Browse, Findings and OMG Trash; in a nested folder the rows are plain names (`victim.bin`) and the path bar reads `~ › work`; the unreadable folder reads `unknown  locked/  unreadable`, not `0.00 B`, and the header says `partial (some folders unreadable)`; the footer, the `m` menu and the `?` help have no delete entry (the help's only line with "delete" is the capability status `permanent delete: unsupported`); `d` shows "Permanent delete is not available in this build." and no question, and the files are still on disk; `␣` adds a file to OMG Trash (`omg trash` lists it) and again removes it, with nothing changed on disk; `r` rescans ("Scan complete: 4 entries, 0 unreadable folders."); a 30×6 window shows "Terminal too small" and a 140×40 window redraws; `q` exits with code 0 and the terminal modes (canonical input, echo) are restored. **Not checked:** Terminal.app, iTerm2, Ghostty or the VS Code terminal (colours, the `⏎`/`⌫`/`␣` glyphs, mouse, real resize and Ctrl-C), the progress display while a long scan runs, the privacy-protection wording in the result line after `r`, and nesting deeper than one folder. One path-bar check at first failed because my script looked in the wrong rows, and the first `␣` check failed because the fixture was outside the throwaway `HOME`, where omg refuses to add anything (the row is marked as locked); both were mistakes in the script, not in omg |
| 9. CI-built binary installed through npm | Pass for the checks below; one item not observed. `Release build` run 41977876172 on `4535902` (green for linux-x64-gnu, linux-arm64-gnu, win32-x64, darwin-arm64 and the dependency inventory); `CLEANUP` for darwin-arm64 is `unsupported`. `scripts/sign-release.js` with a throwaway key verified and signed all four targets; the darwin manifest has `"cleanup":"disabled"`. Installed through `npm install --global --prefix "<path with a space>" --ignore-scripts --offline`: `omg --version` is `0.1.0-beta.1`, `doctor` says `cleanup: unsupported` and `permanent delete: unsupported`, and a scan worked (on the repository folder, not `$HOME`, to avoid privacy questions: 1.33 M estimated entries). `xattr -l` on the installed binary shows only `com.apple.provenance`, no `com.apple.quarantine`; whether macOS showed a "cannot be verified" dialog was not observed. One byte appended to the installed binary: `omg: Binary integrity verification failed`, exit 1; restored, it runs again. The throwaway key, package folder and state folder were deleted. Install smoke test dry-run run 41978189694 passed on seven machines |

## Permanent delete (2026-11-07)

Owner decision 2026-11-07: the macOS build includes permanent delete, declared `development-unqualified`, on APFS only. Linux and macOS share the `unix` module in `cleanup.rs`: same path anchoring, identity, birth time (`st_birthtime` with nanoseconds on macOS), owner, descendant checks and removal through directory descriptors.

Added for macOS, because a plain `rm` would not stop at them:

| Hazard | Behaviour | Basis |
| --- | --- | --- |
| iCloud placeholder (`SF_DATALESS`) | Refused as a target and anywhere inside a folder, checked again while removing | Sources describe a delete in iCloud Drive as removed everywhere; whether `unlink` of a placeholder reaches the cloud was not confirmed, so it is refused |
| Locked or system-protected (`uchg`, `uappnd`, `schg`, `sappnd`, `restricted`) | Refused before anything in the folder is removed | Probe: `unlink` gives `EPERM`, which would stop a folder removal halfway |
| App bundle (`.app`, and anything inside one) | Refused | App Management can deny the removal; a partial removal breaks the app |
| `~/Desktop`, `Documents`, `Downloads`, `Library`, `Movies`, `Music`, `Pictures`, `Public`, `Applications`, `Library/Caches`, `Library/Preferences`, `Library/Application Support` themselves | Refused; items inside are judged on their own | Apps and macOS expect them to exist |
| Inside Desktop or Documents | Warning before confirming | A synced folder shows no attribute that could be checked here, so it is a warning rather than a detection |
| HFS+, exFAT, FAT volumes | Refused | Only APFS was tested |

Probe on APFS (this Mac, macOS 27.2, `/System/Volumes/Data`): birth time did not change on write, `chmod` or rename; `utimes` to a date before the birth time moves it, which makes revalidation refuse (fails closed); no inode number was reused in 2,000 create and delete cycles; `unlink` fails with `EPERM` on a `uchg` file and on a child of a `uchg` folder, and with `EACCES` under a mode 500 folder or an ACL denying delete; a clone has its own inode and `st_nlink` 1, so a clone's shared blocks are invisible to the hard-link accounting; the volume is case and normalization insensitive and lists the name as created.

Verification on that Mac, Rust 1.95.0, Node 22, `TMPDIR` canonical, `OMG_REQUIRE_DISK_IMAGES=1`: default build `cargo test --workspace --locked` 104 passed, 0 failed, 1 ignored, including `tests/cleanup.rs` (25 tests, now also on macOS: revalidation, a replaced file, a swapped folder and link, a protected child added after review, interrupted and partial attempts, hard links, deleting around protected items, locked items, standard folders and app bundles) and `tests/macos.rs` (an HFS+ image refuses delete and keeps the file). Clippy `-D warnings` in both builds; analysis-only tests 78 passed, 0 failed, 1 ignored. `tests/test_terminal.py` through a pseudo-terminal with a throwaway `HOME`: Cancel keeps the file, "Yes, delete permanently" removes only that file, a `.pem` is refused, and a cache is deleted around a protected folder. `release.test.js` 18 passed, 3 skipped. The `unsupported_root` test uses `/proc` and stays Linux only.

Not done: a real iCloud placeholder or synced Desktop and Documents (no such files on this Mac; the flag check is a unit test on the value), a delete with the release binary in a real terminal app or from the CI-built package (`Release build` and `Install smoke test` have not run since this change), `Checks` on the hosted macOS legs, Full Disk Access, App Management denial on a real bundle, a real external drive. Freed space with clones and Time Machine snapshots was not measured.

## Final check, steps 2 to 9, and the daily scan (2026-11-14)

Mac17,9 (Apple silicon), macOS 27.2.1 (26B131), Terminal.app without Full Disk Access, commit `bef8a68`, release build `omg 0.1.0-beta.2`. Each step used its own private state folder under `/private/tmp`.

| Step | Result |
| --- | --- |
| 2. Release build and `doctor` | Pass: `permanent delete: development-unqualified (macOS: APFS only; ...)` and `"cleanup": "development-unqualified"`. |
| 5. Full Disk Access | Not run: turning it on needs System Settings and restarting the terminal. Without it, a home scan is partial: 455,850 entries, 436 unreadable folders, 19,835 links not followed, 85 mandatory exclusions. |
| 6. iCloud placeholders | Not run: no dataless file was found outside Desktop and Documents, which are unreadable without Full Disk Access. |
| 7. External volume | Pass on a mounted HFS+ disk image (`/Volumes/Dia`, removable, read-only): complete, 3,167 entries, 0 errors, 1,266,864,128 bytes for `Dia.app` against 1,268,744,192 bytes used by `df`. Not a USB drive; exFAT/FAT32 on real media and an SMB share not tried. |
| 8. Interface by hand in other terminals | Not run (needs a person); the scripted parts are covered by step 8b and the PTY tests. |
| 8b. Delete in a throwaway home | Pass, driven through a pseudo-terminal with the release binary: Esc keeps `remove-me.txt`; "Yes" deletes only it and `keep-me.txt` stays; `Desktop/note.txt` shows the iCloud warning before the question; `Tools/Thing.app` (application bundle) and `Desktop` (standard macOS folder) are refused with no "Yes"; `locked.txt` (`uchg`) is offered, and the confirmed delete removes nothing: "1 skipped: this is a locked or system-protected item". |
| 9. CI-built binary through npm | Pass: `Release build` run 43556501699 green for all four targets; signed with a throwaway key by `sign-release.js`; the macOS manifest says `"cleanup":"development-unqualified"`; installed into a prefix with a space, `--version`, `doctor` and a home scan (455,895 entries, partial) work; no `com.apple.quarantine`; one changed byte gives "Binary integrity verification failed", and the original binary runs again. |

Daily scan (`omg schedule`) with the real `launchctl`: `enable --hour 3` wrote `~/Library/LaunchAgents/dev.omg.scan.plist` (`plutil -lint` OK) and loaded it with a calendar trigger `Hour 3, Minute 0`, low priority I/O and nice 10; `launchctl kickstart` ran it and a second complete scan was recorded (exit code 0); `enable --hour 4` replaced the loaded job (trigger now `Hour 4`); `disable` unloaded it and removed the file; `status` then says not registered. A plist without omg's marker at the same path was left alone by both `enable` and `disable`. The calendar trigger itself was not waited for.

Finding, fixed the same day: a locked (`uchg`) file was refused only after "Yes". `cleanup::verdict` now reads the item's own flags on macOS, so a locked item or cloud placeholder is refused before any question, like an app bundle; one inside a folder is still caught at delete time (`a_locked_item_stops_the_whole_folder_before_anything_is_removed`).

## Gates not passed

No macOS older than 15, only one real Mac (Apple silicon, macOS 26), no Intel Mac, no Full Disk Access / iCloud / Time Machine behaviour, no real USB drive or SMB share, no Apple notarization or code-signing identity, and the signed packages were assembled here only with a throwaway key (no production key exists). The macOS items in `docs/release.md` stay unchecked.
