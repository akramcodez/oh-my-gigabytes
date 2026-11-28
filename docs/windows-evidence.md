# Windows evidence

Status: **development build: native Windows scanning of NTFS and ReFS, read-only views and permanent delete on NTFS (`development-unqualified`, owner request 2026-10-15).** Recycle Bin is not implemented. Recorded 2026-10-15 on the owner's machine. Plan: windows-implementation-plan.md. Safety invariants S01–S12 still apply.

## Environment

| Item | Value |
| --- | --- |
| OS | Windows 11 Home Single Language 10.0.26200, x64 |
| CPU / RAM | Intel Core i5-13500H, 15.7 GB |
| Volumes | C: NTFS 200 GB; D: NTFS 175 GB (WSL virtual disk, 0.15 GB free at last check); F: FAT32 3 GB |
| Terminal | Windows Terminal 1.24 with Windows PowerShell 5.1 (`powershell.exe`) |
| Rust | `1.95.0-x86_64-pc-windows-gnu` (the pinned version, GNU ABI) |
| Linker | MinGW-w64 WinLibs, gcc 16.1.0 (UCRT), copied to `C:\src\mingw` |

Not used: the pinned MSVC toolchain. No MSVC linker or Windows SDK is installed (the Visual Studio 2019 Build Tools shell has no C++ toolset). CI's `windows-latest` runs MSVC, so the MSVC build is first verified by CI.

Build location: source mirror `C:\src\omg`, build output `C:\src\omg-target`. Built from a copy on C:, not from `\\wsl.localhost`.

## Commands and results

| Command | Result |
| --- | --- |
| `cargo +1.95.0-x86_64-pc-windows-gnu build -p omg-cli --locked` | pass |
| `cargo ... clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo ... test --workspace --locked` | pass (all suites; 13 Windows integration tests, 6 of them for deletion) |
| `cargo ... check -p omg-core --no-default-features --locked` | pass (analysis-only build) |
| `omg --version` / `omg doctor` | reports `platform: windows`, `cleanup: unsupported` |
| `omg --state-dir C:\src\omg-state scan --root C:\src\omg` | complete, 134 entries, 0 errors |
| `omg ... scan --root "C:\Users\<user>"` (release build) | complete, 1,121,127 entries, 0 errors, 137 excluded, 44–49 s, two runs |
| `omg ... status`, `tree`, `findings` | succeed; findings include npm, pnpm, uv and Cargo caches under their Windows locations |
| TUI in Windows Terminal (Overview, Browse, Findings) | renders; paths shown with `\` and `~` for the home folder; keys `1`–`4`, `q` work |

Linux gate, unchanged by this work: `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace --locked`, analysis-only clippy and tests, the PTY smoke test and `release.test.js` (12/12) all pass.

## Measured

| Measurement | Method | Result |
| --- | --- | --- |
| Fixture totals | scan `C:\src\omg`; `std::fs` oracle in `tests/windows.rs` and PowerShell | 108 files, 26 directories, 996,807 apparent bytes: equal |
| Reparse points | scan vs `dir /s /aL` on the profile | 92 by both; all recorded as "symlink not followed" |
| Profile scan throughput | release build, first scan into an empty state | ~23,000 entries/s (1,121,127 in ~46 s), no errors |
| Debug build, same scan | for comparison only | 1,121,079 entries in 76 s |

Throughput is one machine, one run per build, with Windows Defender active. It is not a representative-machine benchmark and is not a performance claim.

## Behaviour verified

- Junctions are recorded, never followed, and nothing below them is indexed.
- A junction as the scan root is refused.
- Hard links are listed with one identity (`volume:file-id`).
- Paths longer than MAX_PATH are scanned.
- Names with Unicode and an unpaired surrogate round-trip through the WTF-8 store.
- OneDrive is excluded by name, so its contents are not opened.

## Permanent delete on NTFS

Same single path as Linux (`cleanup::Index::prepare_delete` → `delete_permanently`), same gates: confirmed one-use plan, home only, hard protection, repository rule, recorded creation time.

| Step | Windows mechanism |
| --- | --- |
| Hold the target | Opened with `DELETE` access and **no delete sharing**, reparse points visible: nobody can rename, replace or delete it while OMG holds it |
| Revalidate | Through that handle: `volume:file-id` identity and creation time must match the saved item; final path must equal the expected path; owner must be you (the token's user, or its default owner when elevated, such as Administrators: the Windows form of `st_uid == euid`); not a cloud or offline placeholder |
| Descendants | Fresh walk before removal: sensitive and protected names, cloud placeholders, volume mount points and unknown link types stop the delete. Junctions and symbolic links inside are removed as links, never followed |
| Remove | `FileDispositionInfoEx` (delete + POSIX semantics + ignore read-only) on each verified handle, files first, then each emptied folder through its own handle |
| Hard links | Link counts are read at removal; a removed name whose data has other names sets `reconcile_needed` |

Filesystems: NTFS only. ReFS (untested) and FAT32/exFAT (no stable file ids) are refused by `capability::permanent_delete`.

Read-only files are removed like any other file, matching Linux, where a file's mode does not govern unlinking it.

Tests (`crates/omg-core/tests/windows.rs`, module `deletion`, run on this machine with `TMP=C:\src\omg-tmp`): confirmed folder and file deleted, including read-only contents; a replaced file is skipped and the replacement survives; a junction inside a deleted folder is removed while its target survives; a folder swapped for a junction never leads outside; a protected child inserted after validation stops the delete; a file held open without delete sharing is skipped as in use; removing one hard-link name keeps the data and flags totals. All pass. The tests skip, with a message, when the temp folder is inside a Git repository.

Release manifest: Windows full builds now declare `analysis: development-windows`, `cleanup: development-unqualified`, `service: disabled`; `assemble-release.js` checks the claim against a Windows binary's `doctor` when run on Windows. `verify.js` and `release.test.js` changed with it.

## Repository rule (finding for the owner, resolved)

The owner's home folder is a Git repository (`C:\Users\SK AKRAM\.git` exists; still true on 2026-11-02). Under the original rule almost everything in the home was "inside an active repository". Resolved in commit `70cd51c`: `RepositoryMemo::is_repository` treats a repository at or above the home folder as a dotfiles repository, so it does not make the home folder's contents repository data; repositories below the home folder keep the rule.

## Owner-only state directory (2026-10-19)

`platform::windows_state` is the counterpart of the Unix "owned by you, mode 0700" check. A state directory OMG creates gets a protected access list (`D:PAI(A;OICI;FA;;;<you>)(A;OICI;FA;;;SY)`: no inherited entries, full control for you and SYSTEM, inherited by files inside). Every open verifies that you (or your token's default owner, for an elevated administrator) own the directory and that no access entry grants anything to anyone else. SYSTEM, Administrators, CREATOR OWNER and OWNER RIGHTS are accepted. A directory that fails is refused with the `icacls` command that fixes it, and `omg doctor` refuses it too without changing anything. `platform/windows.rs` stays read-only; the new module touches only OMG's own state directory.

Existing state directories created by earlier builds inherit their parent's access list. Under `%LOCALAPPDATA%` that list is already private, but a folder under a drive root such as `C:\src\omg-state` usually inherits "Authenticated Users" and is now refused until the `icacls` fix is applied.

Verified: `tests/windows.rs` module `state_acl` (a created directory lists no Everyone/Users/Authenticated Users; a directory granted to Everyone is refused, `omg doctor` does not repair it, and it is accepted again after the grant is removed). Run natively on the owner's PC (Rust 1.95.0 GNU toolchain from a copy on C:, 15 tests passed) and on `windows-2022`, `windows-latest` and `windows-11-arm` (run 39111223427). `cargo clippy -D warnings` and `cargo fmt --check` were clean natively in both build modes.

## Hosted CI and release binary (2026-10-19)

- CI legs: `windows-2022`, `windows-latest` (MSVC) and `windows-11-arm` pass fmt, clippy and tests in the default and analysis-only builds, plus the Node release tests (not on Arm).
- The released `omg.exe` is built with a static C runtime (`.cargo/config.toml`). Its import table lists only `KERNEL32`, `USER32`, `advapi32`, `ntdll`, `bcryptprimitives` and `api-ms-win-core-synch`; no Visual C++ runtime DLL (`objdump -p` on the file from release run 39108897938).
- The signed `win32-x64` package installed, ran, scanned, reinstalled and uninstalled on `windows-2022` and `windows-latest` (smoke run 39196125718). Authenticode signing is not done, so SmartScreen may warn on a manual download.

## Native check after the macOS merge, and the delete flow through ConPTY (2026-11-02)

Environment: the owner's PC above (Windows 11 Home 10.0.26200, x64), Rust `1.95.0-x86_64-pc-windows-gnu` with the MinGW linker in `C:\src\mingw`, Python 3.13.3 with `pywinpty` 2.0.15 in `C:\src\venv`, Node 24.13.0. Tree: `origin/main` at `74f4d8f` (the macOS work) plus the uncommitted 2026-10-24 delete-in-release work and the fixes below, copied to `C:\src\omg`. `TMP` was `C:\src\omg-tmp` (outside the profile and outside any Git repository). Built with `CARGO_INCREMENTAL=0` and `CARGO_PROFILE_{DEV,TEST}_DEBUG=0` because C: had 0 bytes free at the start (see Test artifacts). Script: `C:\src\omg-full.ps1`.

| Step | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings`, and with `--no-default-features` | pass, both |
| `cargo test --workspace --no-default-features --locked` | 49 passed, 0 failed, 1 ignored |
| `cargo test --workspace --locked` | 56 passed, 0 failed, 1 ignored; `tests/windows.rs` 16 of 16, including the 7 deletion tests |
| `python -m unittest discover -s tests -p test_terminal_windows.py` with the debug and with the `--release` default-feature `omg.exe` | 3 of 3 each (start/quit, and the two delete tests below) |
| `node --test packages/oh-my-gigabytes/test/release.test.js` | 21 of 21 |
| release `omg.exe doctor` | `permanent delete: development-unqualified (Windows: NTFS only; ...)` |

New ConPTY tests (`tests/test_terminal_windows.py`, also run by CI's Windows x64 legs), each in a throwaway folder given to `omg` as `USERPROFILE` and `LOCALAPPDATA`, driving the real interface through ConPTY and reading a reconstructed screen (`tests/terminal_screen.py`, shared with the Linux PTY tests):

- `test_delete_removes_only_the_confirmed_file_and_refuses_protected_ones`: Browse, `d` on a 3 MiB file, "Are you sure?" names it and defaults to Cancel; Esc leaves the file and the message "Nothing was deleted" stays; `d`, "Yes, delete permanently" removes exactly that file and leaves its neighbour; `d` on `secret.pem` is refused ("cannot add to OMG Trash") with no delete offered.
- `test_a_cache_holding_a_protected_folder_is_deleted_around_it`: uv's Windows cache (`%LOCALAPPDATA%\uv\cache`) holding a `credentials` package folder cannot be deleted whole; "Delete everything except the protected items" plans around it, asks again, and deletes the rest while the protected folder survives.

This closes the gap "the confirmed removal through the interface was never run on Windows" for the GNU-toolchain release build on NTFS. The shipped binary is built with MSVC in CI; CI runs these tests with its debug MSVC build, not the release one (both passed on windows-2022 and windows-latest, `Checks` run 41395760636).

Found and fixed by this run:

- **Browse showed whole paths on Windows.** Rows read `work\victim.bin` instead of `victim.bin`, and the path bar showed `AppData\Local` as one crumb, because both split the display form of the path (which uses `\` on Windows) on `/`. They now split the stored path on `/` and convert each part. Unit test `nested_folders_show_names_and_one_crumb_per_folder` (runs on every platform).
- **Cancel's message was replaced on every platform.** Esc at "Are you sure?" set "Cancelled. Nothing was deleted." and sent two requests; the worker's reply to the first ("Removed from OMG Trash.", for the item `d` had staged) arrived afterwards and replaced it. Nothing was deleted, but the message said otherwise. Cancel is now one request (`CancelPlan`) whose only reply is the cancel message. Unit test `escape_at_the_question_cancels_in_one_request_and_keeps_its_message`; the Linux PTY and the ConPTY delete tests now wait a second after Esc and check the message is still there (the Linux one fails on the old binary, whose screen ends with "Removed from OMG Trash.").
- **Two tests failed with `TMP` outside the profile.** `tests/migrations.rs` and `tests/analysis_only.rs` opened state folders in plain temp folders, which there inherit an access list open to other users, and omg correctly refused them. They now call `platform::windows_state::protect`, as `tests/windows.rs` does. CI did not see this because its `TMP` is inside the runner's profile.

## Delete qualification script (2026-11-15)

`scripts/qualify-delete.py --binary target/debug/omg.exe --root %RUNNER_TEMP%` drives the interface through ConPTY in throwaway home folders on NTFS. On `windows-2022` (Server 2022) and `windows-latest` (Server 2025), run 43678086940: 7 of 7 passed: Esc keeps the file; "Yes" deletes only the confirmed file; a `.pem` is refused with no question; a 220-item folder is deleted whole; links inside a deleted folder are removed as links and their targets keep their content; deleting one name of a hard-linked file keeps the other; a file replaced between the question and "Yes" is skipped. Symbolic links were created (the runners are administrators); on an account without that right the link scenario is reported as skipped. Not done: the same with the MSVC release binary from `release.yml` on the owner's PC.

## The signed 0.1.0 release package on a Windows 11 PC (2026-11-21)

Owner's laptop: Windows 11 Home 25H2 (build 10.0.26200.7462), AMD64, C: and D: NTFS, F: FAT32. Standard account without Developer Mode, so it cannot create symbolic links. PowerShell 5.1, Node 24.13.0, Python 3.13.3 with `pywinpty` 2.0.15. Tests from a clone of `main` at `739f959`. Package: `oh-my-gigabytes-offline-win32-x64-0.1.0.tgz` from `Release build` run 44874343258, signed with the production key; its SHA-256 `a0735f24...3052` equals the line in `SHA256SUMS`.

| Step | Result |
| --- | --- |
| Signature | Pass: `verify-release.js --target win32-x64 --version 0.1.0` verifies with key `01deacfe...9adc0`; a wrong target is refused ("Release version/target/schema mismatch"). |
| Install | Pass: `npm install -g --prefix ... --ignore-scripts`, also into a prefix with a space (through `omg.cmd` and `omg.ps1`); `omg.exe` matches the manifest hash. |
| `--version`, `doctor` | Pass: `omg 0.1.0`; "permanent delete: development-unqualified (Windows: NTFS only; ReFS and FAT32 refused; cloud placeholders never deleted)"; `doctor` did not create the state folder. |
| State folder | Pass: after the first scan `icacls` lists only the user and SYSTEM. |
| Refusals | Pass: FAT32 `F:\` ("FAT32 and exFAT volumes cannot be scanned on Windows yet ..."), a junction ("scan root is a link or mount point; scan the folder it points to instead"), `\\localhost\C$\...` ("only local drive paths are supported"); each exits 1. A symbolic link root was not tried (no privilege). |
| Profile scan | Pass: complete, 1,098,681 entries, 0 errors, 141 excluded, 16.4 s (26 s with startup); 65.4 GiB indexed; index 290 MiB. 172 findings (159 review, 9 info, 4 protected), 30.6 GiB explained. The ten junctions at the profile root are not followed; `OneDrive` is excluded by name and never opened. |
| `status`, `tree`, `findings`, `trends`, `history`, `schedule status` | Pass: all exit 0; every finding path is relative to the profile; `schedule status` says not registered. |
| ConPTY delete tests | Pass: `test_terminal_windows.py` with `OMG_BINARY` set to the installed `omg.exe`: 3 of 3, none skipped. |
| `qualify-delete.py` | Pass: 6 of 7, the link scenario skipped ("this account cannot create symbolic links"). |
| Interface by hand | Pass, by the owner, in Windows Terminal and in the console host: drawing, resizing, the views, the delete question and `q` restoring the terminal. |
| Tamper | Pass: one byte appended to `omg.exe` gives "omg: Binary integrity verification failed" (exit 1); a reinstall runs again. |

Not done: a symbolic link root and the link-inside-a-folder delete scenario on this account (they need Developer Mode), OneDrive cloud-only placeholders (none on this PC). Found, not fixed in 0.1.0: `omg tree` on the excluded `OneDrive` folder itself shows 0 bytes with `incomplete: false`, although it holds 1,190 files; the listing of the profile shows it as excluded.

## The 0.2.0 CI binary before signing (2026-11-27)

Same PC and account as the 0.1.0 check. Binary: `omg.exe` from `Release build` run 45944007561 on `06919e2`, unsigned and run directly; its SHA-256 `63dbdf6b...0306` equals the build's `SHA256SUMS`. Tests from a clone at `06919e2`. Signed 0.2.0 packages did not exist yet, so the launcher, signature and tamper steps were not run.

| Step | Result |
| --- | --- |
| `--version`, `doctor` | Pass: `omg 0.2.0`; "permanent delete: development-unqualified (Windows: NTFS only; ReFS and FAT32 refused; cloud placeholders never deleted)"; "auto delete: off; job not registered"; the daily scan job is reported, not registered. |
| State folder | Pass: `icacls` lists only the user and SYSTEM. |
| Refusals | Pass: FAT32 `F:\`, a junction, `\\localhost\C$\...`, with the same messages as 0.1.0; each exits 1. |
| Profile scan | Pass: complete, 726,828 entries, 0 errors, 128 excluded, 14.5 s (23 s with startup), 10.2 s on a rescan; 55.12 GiB indexed; index 378 MiB; 123 findings (112 review, 6 info, 5 protected), 21.4 GiB explained. The profile shrank since 2026-11-21: 0.1.0 scanned the same profile the same day to 726,829 entries and 55.12 GiB. |
| `status`, `tree`, `findings`, `trends`, `history`, `grew`, `unexplained` | Pass: all exit 0. |
| Fixed 0.1.0 issues | Pass: `tree` on `OneDrive` gives sizes `null`, `excluded`, `incomplete: true`; the rescan's `estimated_total` is the previous count (726,828). |
| `schedule status`, `auto-delete status` | Pass: not registered, `executable_missing: false`; auto delete off, nothing due. |
| ConPTY delete tests | Pass: `test_terminal_windows.py` with `OMG_BINARY` set to the CI `omg.exe`: 3 of 3, none skipped. |
| `qualify-delete.py` | Pass: 6 of 7, the link scenario skipped ("this account cannot create symbolic links"). |

Not done: the signed 0.2.0 package (install, signature, tamper), turning auto delete on and off, the interface by hand (small terminals, scan animation), a symbolic link root.

## Known gaps (not done, not claimed)

- **Hard-link counts.** Enumeration does not read link counts, so each Windows name counts once. Data reachable by two names is double-counted in folder totals until link counts are read (plan W02).
- **Containment.** Each directory is checked by final path and file ID, which refuses a junction swapped into an intermediate component. Relative `NtCreateFile` opens (the stronger design in the plan) are not used.
- **State directory ACLs.** Done 2026-10-19: see "Owner-only state directory" below.
- **Cloud placeholders.** Placeholder detection uses attributes and reparse tags. It is not tested against a live OneDrive sync root. `OneDrive` in the profile is excluded by name; since 0.2.0 `omg tree` on that folder itself shows its size as unknown and `incomplete: true` (checked 2026-11-27).
- **Drive root and system folders.** Scanning `C:\` as a whole is untested. `System Volume Information` and `$Recycle.Bin` are excluded by name.
- **Progress estimate.** No used-inode count is available on Windows, so the ETA is unavailable.
- **Not implemented:** drives view, Recycle Bin size, WSL/Docker virtual-disk findings (M2); background service (M4); Recycle Bin deletion; Authenticode signing (W09). The ConPTY tests (W11) exist since 2026-10-19 and cover delete since 2026-11-02.
- **Deletion through the TUI** is automated through ConPTY since 2026-11-02 (see above). Done on 2026-11-21 with the signed MSVC release package on the owner's PC, through ConPTY and by hand in Windows Terminal and the console host.
- **Launcher Ctrl+C.** `bin/omg.js` no longer force-kills the child on Windows. This was not run through a packaged binary.
- **Git Bash (mintty).** Reported by a user on 2026-11-27: in Git Bash's default window the arrow keys did nothing and keys such as `2` and `3` were echoed. mintty gives programs a named pipe instead of a console; Rust reports that pipe as a terminal, so the interface started but read keys from a console that never receives them. Since this fix the interface refuses such a pipe and suggests `winpty omg`, Git Bash in Windows Terminal, or PowerShell. Covered by `interface_refuses_a_mintty_pipe_and_explains` in `crates/omg-cli/tests/cli.rs` (Windows CI legs), which hands `omg` pipes named like mintty's. Not run by hand in mintty, with `winpty`, or in Git Bash inside Windows Terminal.
- **TUI.** Checked by screenshots of Overview, Browse and Findings, and quit with `q`. Ctrl+C and window-close behaviour were not exercised.
- **FAT32 and exFAT** are refused. The virtual-disk matrix (`scripts/win-vhd-matrix.ps1`, run on `windows-2022` and `windows-latest`, 2026-10-19) found that scanning such a volume failed with "The parameter is incorrect" (error 87), because the scanner's file identity and directory listing use information classes that only NTFS and ReFS provide. `open_root` now refuses them with "FAT32 and exFAT volumes cannot be scanned on Windows yet". Supporting them needs a separate enumeration path and is not part of the beta. (macOS scans FAT32 and exFAT.)
- **ReFS** scans correctly on a real ReFS volume: the virtual-disk matrix on `windows-2022` and `windows-latest` (Server 2022 and 2025, run 39201129096, 2026-10-19) built NTFS and ReFS volumes from virtual disks and scanned each from its root: 7 entries, 200,005 bytes, equal to an independent count, hard link counted once. Windows 11 Home cannot format ReFS, so this is hosted-runner evidence only.

## Test artifacts

Removed after use: screenshots, probe scripts, the link-test crate, scan state directories and the copied index. Kept, for the owner to remove when no longer needed: `C:\src\omg` (1 MB source mirror), `C:\src\omg-target` (build output), `C:\src\mingw` (about 0.9 GB MinGW toolchain), `C:\src\venv` (Python with `pywinpty`) and `C:\src\omg-state-r12` (0.7 GB index from the 2026-10-19 profile measurement).

2026-11-02: C: had 0 bytes free when the native run started, and the first attempt failed with "not enough space on the disk". `C:\src\omg-target` had grown to 3.6 GB of stale debug output; it was emptied with `cargo clean` (3.6 GiB) and rebuilt without debug info. C: then had 3.4 GB free, and 11.9 GB when the run finished; that later increase came from something else on the PC.

## Auto delete

Auto delete (D16, [evidence.md](evidence.md)) registers the Task Scheduler task `omg\\auto-delete` (every 6 hours) and notifies through a PowerShell toast. It is compiled and its unit tests run on the Windows CI runners only; no native run, toast or battery check has been seen on a real PC.
