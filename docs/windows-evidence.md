# Windows evidence

Status: **development build: native Windows scanning of NTFS and ReFS, read-only views and permanent delete on NTFS (`development-unqualified`, owner request 2026-10-15).** Recycle Bin is not implemented. Recorded 2026-10-15 on the owner's machine. Plan: [windows-implementation-plan.md](windows-implementation-plan.md). Safety invariants S01–S12 still apply.

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

## Repository rule (finding for the owner)

The owner's home folder is a Git repository (`C:\Users\SK AKRAM\.git` exists). The existing policy walks up from each entry to find a repository, so under the current rule almost everything in the home is "inside an active repository". Only build and dependency folders stay deletable there. This is existing policy, not Windows-specific, and it is not changed here. Decide before the deletion milestone whether the home-as-repository case should be treated differently.

## Owner-only state directory (2026-10-19)

`platform::windows_state` is the counterpart of the Unix "owned by you, mode 0700" check. A state directory OMG creates gets a protected access list (`D:PAI(A;OICI;FA;;;<you>)(A;OICI;FA;;;SY)`: no inherited entries, full control for you and SYSTEM, inherited by files inside). Every open verifies that you (or your token's default owner, for an elevated administrator) own the directory and that no access entry grants anything to anyone else. SYSTEM, Administrators, CREATOR OWNER and OWNER RIGHTS are accepted. A directory that fails is refused with the `icacls` command that fixes it, and `omg doctor` refuses it too without changing anything. `platform/windows.rs` stays read-only; the new module touches only OMG's own state directory.

Existing state directories created by earlier builds inherit their parent's access list. Under `%LOCALAPPDATA%` that list is already private, but a folder under a drive root such as `C:\src\omg-state` usually inherits "Authenticated Users" and is now refused until the `icacls` fix is applied.

Verified: `tests/windows.rs` module `state_acl` (a created directory lists no Everyone/Users/Authenticated Users; a directory granted to Everyone is refused, `omg doctor` does not repair it, and it is accepted again after the grant is removed). Run natively on the owner's PC (Rust 1.95.0 GNU toolchain from a copy on C:, 15 tests passed) and on `windows-2022`, `windows-latest` and `windows-11-arm` (run 39111223427). `cargo clippy -D warnings` and `cargo fmt --check` were clean natively in both build modes.

## Hosted CI and release binary (2026-10-19)

- CI legs: `windows-2022`, `windows-latest` (MSVC) and `windows-11-arm` pass fmt, clippy and tests in the default and analysis-only builds, plus the Node release tests (not on Arm).
- The released `omg.exe` is built with a static C runtime (`.cargo/config.toml`). Its import table lists only `KERNEL32`, `USER32`, `advapi32`, `ntdll`, `bcryptprimitives` and `api-ms-win-core-synch`; no Visual C++ runtime DLL (`objdump -p` on the file from release run 39108897938).
- The signed `win32-x64` package installed, ran, scanned, reinstalled and uninstalled on `windows-2022` and `windows-latest` (smoke run 39196125718). Authenticode signing is not done, so SmartScreen may warn on a manual download.

## Known gaps (not done, not claimed)

- **Hard-link counts.** Enumeration does not read link counts, so each Windows name counts once. Data reachable by two names is double-counted in folder totals until link counts are read (plan W02).
- **Containment.** Each directory is checked by final path and file ID, which refuses a junction swapped into an intermediate component. Relative `NtCreateFile` opens (the stronger design in the plan) are not used.
- **State directory ACLs.** Done 2026-10-19: see "Owner-only state directory" below.
- **Cloud placeholders.** Placeholder detection uses attributes and reparse tags. It is not tested against a live OneDrive sync root.
- **Drive root and system folders.** Scanning `C:\` as a whole is untested. `System Volume Information` and `$Recycle.Bin` are excluded by name.
- **Progress estimate.** No used-inode count is available on Windows, so the ETA is unavailable.
- **Not implemented:** drives view, Recycle Bin size, WSL/Docker virtual-disk findings (M2); background service (M4); Recycle Bin deletion; npm `win32-x64` package and Authenticode signing (W09); ConPTY smoke test (W11).
- **Deletion through the TUI** was reached up to the confirmation dialog in Windows Terminal (plan accepted, "Are you sure? 1 item(s)"). The confirmed removal itself is verified by the integration tests, which call the same backend functions the TUI calls; an automated keypress run of the final `y` was abandoned because desktop focus moved.
- **Launcher Ctrl+C.** `bin/omg.js` no longer force-kills the child on Windows. This was not run through a packaged binary.
- **TUI.** Checked by screenshots of Overview, Browse and Findings, and quit with `q`. Ctrl+C and window-close behaviour were not exercised.
- **FAT32 and exFAT** are refused. The virtual-disk matrix (`scripts/win-vhd-matrix.ps1`, run on `windows-2022` and `windows-latest`, 2026-10-19) found that scanning such a volume failed with "The parameter is incorrect" (error 87), because the scanner's file identity and directory listing use information classes that only NTFS and ReFS provide. `open_root` now refuses them with "FAT32 and exFAT volumes cannot be scanned on Windows yet". Supporting them needs a separate enumeration path and is not part of the beta. (macOS scans FAT32 and exFAT.)
- **ReFS** scans correctly on a real ReFS volume: the virtual-disk matrix on `windows-2022` and `windows-latest` (Server 2022 and 2025, run 39201129096, 2026-10-19) built NTFS and ReFS volumes from virtual disks and scanned each from its root: 7 entries, 200,005 bytes, equal to an independent count, hard link counted once. Windows 11 Home cannot format ReFS, so this is hosted-runner evidence only.

## Test artifacts

Removed after use: screenshots, probe scripts, the link-test crate, scan state directories and the copied index. Kept, for the owner to remove when no longer needed: `C:\src\omg` (1 MB source mirror), `C:\src\omg-target` (about 1.9 GB of build output) and `C:\src\mingw` (about 0.9 GB MinGW toolchain).
