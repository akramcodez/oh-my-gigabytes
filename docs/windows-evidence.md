# Windows evidence

Status: **development build, native Windows scanning and read-only views.** Deletion is not available on Windows and is not claimed. Recorded 2026-10-15 on the owner's machine. Plan: [windows-implementation-plan.md](windows-implementation-plan.md). Safety invariants S01–S12 still apply.

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
| `cargo ... test --workspace --locked` | pass (all suites; 7 Windows integration tests) |
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
- Saving to OMG Trash is refused under the repository rule, or allowed and then refused for deletion. Either way the file stays in place (see *Repository rule* below).
- OneDrive is excluded by name, so its contents are not opened.

## Repository rule (finding for the owner)

The owner's home folder is a Git repository (`C:\Users\SK AKRAM\.git` exists). The existing policy walks up from each entry to find a repository, so under the current rule almost everything in the home is "inside an active repository". Only build and dependency folders stay deletable there. This is existing policy, not Windows-specific, and it is not changed here. Decide before the deletion milestone whether the home-as-repository case should be treated differently.

## Known gaps (not done, not claimed)

- **Hard-link counts.** Enumeration does not read link counts, so each Windows name counts once. Data reachable by two names is double-counted in folder totals until link counts are read (plan W02).
- **Containment.** Each directory is checked by final path and file ID, which refuses a junction swapped into an intermediate component. Relative `NtCreateFile` opens (the stronger design in the plan) are not used.
- **State directory ACLs.** The owner-only ACL check (W03) is not implemented. The Unix mode check does not apply on Windows.
- **Cloud placeholders.** Placeholder detection uses attributes and reparse tags. It is not tested against a live OneDrive sync root.
- **Drive root and system folders.** Scanning `C:\` as a whole is untested. `System Volume Information` and `$Recycle.Bin` are excluded by name.
- **Progress estimate.** No used-inode count is available on Windows, so the ETA is unavailable.
- **Not implemented:** drives view, Recycle Bin size, WSL/Docker virtual-disk findings (M2); background service (M4); deletion (M5); npm `win32-x64` package and Authenticode signing (W09); ConPTY smoke test (W11).
- **Launcher Ctrl+C.** `bin/omg.js` no longer force-kills the child on Windows. This was not run through a packaged binary.
- **TUI.** Checked by screenshots of Overview, Browse and Findings, and quit with `q`. Ctrl+C and window-close behaviour were not exercised.
- **ReFS and FAT32** are recognised but not tested on a real volume.

## Test artifacts

Removed after use: screenshots, probe scripts, the link-test crate, scan state directories and the copied index. Kept, for the owner to remove when no longer needed: `C:\src\omg` (1 MB source mirror), `C:\src\omg-target` (about 1.9 GB of build output) and `C:\src\mingw` (about 0.9 GB MinGW toolchain).
