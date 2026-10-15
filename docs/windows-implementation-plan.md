# Windows implementation plan

Status: **M0 and M1 built and verified on the owner's machine; M2 partly; M3–M5 not started.** Evidence and gaps are in [windows-evidence.md](windows-evidence.md). Written 2026-10-15 from the code at `04ea175` and a live inspection of the owner's Windows machine. Certification is not claimed. [safety.md](safety.md) invariants S01–S12 apply unchanged. Where this plan and the safety baseline disagree, the safety baseline wins.

| Work package | State (2026-10-15) |
| --- | --- |
| W00 environment | Done with a GNU toolchain and a copied MinGW linker. MSVC not installed. |
| W01 lossless paths | Done: WTF-8 store with `/`, displayed with `\`. Case-insensitive names. |
| W02 NTFS scanner | Done with deviations: final-path and file-ID checks instead of `NtCreateFile`; link counts not read. |
| W03 state and locks | Partly: locks use std `try_lock`; owner-only ACL check not done. |
| W04 drives | Not done (volume numbers are shown for the scan root only). |
| W05 findings | Partly: Windows cache locations added. WSL/Docker VHDX and Recycle Bin not done. |
| W06 terminal | Partly: TUI verified in Windows Terminal; launcher no longer force-kills on Ctrl+C. |
| W07 service | Not started. |
| W08 deletion | Not started; refused on Windows, covered by a test. |
| W09 packaging | Not started. |
| W10 performance | Baseline measured (release, profile scan). |
| W11 CI and tests | Partly: Windows integration tests added and run on the host. ConPTY test not done. |

Goal: `omg` runs natively on Windows 11 in Windows Terminal, with fast NTFS scanning, a correct TUI, and honest disk numbers. Cleanup comes last, and only after its own safety gate.

## 1. Where things stand

### 1.1 The code today (verified by reading)

| Area | Windows behaviour now | Source |
| --- | --- | --- |
| Build | CI `windows-latest` compiles and tests stubs only | `.github/workflows/check.yml` |
| State / index | `Index::open` bails: "native scanning/state support currently requires Linux" | `index.rs:138` |
| Scanner | Bails: "native scanner not certified on this platform" | `scanner.rs:22` |
| Paths | `platform::raw`/`path` bail: "native Windows paths are not certified" | `platform.rs:57,69` |
| `doctor` | Prints "this platform is not certified" | `main.rs:224` |
| Service | `start`/`stop`/`service` refused off Linux | `main.rs:217` |
| Cleanup | Linux-only module; capability `unsupported` | `cleanup.rs`, `capability.rs` |
| TUI | crossterm + ratatui (Windows-capable). Already filters `KeyEventKind::Press`, detects `WT_SESSION` for truecolor, handles Ctrl+C as a key in raw mode | `tui/mod.rs:904,649`, `tui/theme.rs:34` |
| Catalog roots | Unix layouts only (`~/.cache/pip`, `~/.npm/_cacache`, ...) | `catalog.rs:25` |
| npm launcher | On Windows, `child.kill('SIGINT')` force-terminates the child (Node maps every signal to `TerminateProcess`). A Ctrl+C during `omg scan` bypasses OMG's graceful cancel | `packages/oh-my-gigabytes/bin/omg.js:33` |

In short, every useful command fails on Windows today. Only `--version` and `doctor` work.

### 1.2 The owner's machine (observed 2026-10-15)

| Item | Value | Consequence |
| --- | --- | --- |
| OS | Windows 11 Home Single Language 10.0.26200, x64 | Target Windows 11 first; Home has no Hyper-V `Optimize-VHD` |
| CPU / RAM | i5-13500H, 15.7 GB | Parallel enumeration is worth it |
| Terminal | Windows Terminal 1.24; Windows PowerShell 5.1 only (no PowerShell 7) | Test in WT + PowerShell 5.1 + cmd; 5.1 decodes piped output with the OEM code page |
| Volumes | C: NTFS 200 GB, **15 GB free**; D: NTFS 175 GB, **0 GB free**; F: FAT32 3 GB | Drive overview is a first-class feature; FAT32 must be analysis-only |
| WSL | WSL 2.4.13; `D:\WSL\Ubuntu\ext4.vhdx` is **174 GB**, but Linux inside reports **77 GB used** | About 97 GB is reclaimable by compacting the virtual disk. This is why D: is full |
| OneDrive | `C:\Users\SK AKRAM\OneDrive` | Cloud placeholders must never be hydrated |
| User name | Contains a space (`SK AKRAM`) | Already breaks the WinLibs MinGW linker (`cannot find AKRAM/...`). Every path we build must be quoted and verbatim |
| Long paths | `LongPathsEnabled=1` | Still embed `longPathAware` and use `\\?\` paths; other machines may not have it |
| Execution policy | CurrentUser `RemoteSigned`, LocalMachine `Restricted` | npm's `omg.ps1` shim works here, but is blocked on default machines |
| Rust | rustup with 1.95.0/1.97.1 MSVC and 1.97.1 GNU toolchains | Toolchains exist; linkers don't (below) |
| C++ toolchain | VS 2019 Build Tools shell only: **no MSVC toolset, no Windows SDK** | Native MSVC builds cannot link |
| PATH | `C:\Program Files\Git\usr\bin` provides a coreutils `link.exe` that shadows MSVC `link.exe` | Observed: `link: extra operand ...` |
| Developer Mode | Off | Unprivileged symlink creation in tests is unavailable |

Native build attempts on 2026-10-15: MSVC failed (Git's `link.exe`, no MSVC toolset). GNU failed (MinGW `ld` broke on the space in the user name). No Windows binary was run.

## 2. Owner decisions needed

| ID | Question | Recommendation |
| --- | --- | --- |
| DW1 | Make Windows a supported target now? `plans.md` currently prioritises Linux and macOS | Yes for analysis (M1–M3); record the change in `plans.md` |
| DW2 | Cleanup on Windows: none, Recycle Bin, or permanent delete? | None until M5; then permanent delete-by-handle (see W08), Recycle Bin only after its race is solved |
| DW3 | Filesystems | NTFS full; ReFS/Dev Drive analysis; FAT32/exFAT analysis-only; network and removable refused at first |
| DW4 | Background service and toasts in this plan? | Service yes (M4); toasts after an AppUserModelID decision |
| DW5 | Authenticode code-signing certificate | Needed before public release; the Ed25519 manifest does not satisfy SmartScreen or Defender reputation |

## 3. Milestones

| Milestone | Delivers | Work packages |
| --- | --- | --- |
| M0 | A working Windows dev setup on this machine | W00 |
| M1 | `omg scan`, `status`, `tree`, `findings` and the TUI work on NTFS in Windows Terminal; deletion stays disabled | W01–W04, W06 |
| M2 | Windows-aware findings: dev caches, WSL and Docker virtual disks, Recycle Bin, drive overview | W05 |
| M3 | npm install works on a clean Windows machine | W09, W10 |
| M4 | Scan-only background service via Task Scheduler | W07 |
| M5 | Explicit deletion on NTFS, behind its own gate | W08 |

W11 (CI and tests) runs alongside every milestone.

## 4. Work packages

### W00: Development environment on this machine

1. **Free space first.** D: has 0 bytes free because of the WSL disk. Recommended (owner runs it; OMG never does): inside WSL run `sudo fstrim -av`, then in PowerShell run `wsl --shutdown` and `wsl --manage Ubuntu --set-sparse true`. Re-check the `ext4.vhdx` size. `diskpart` `compact vdisk` is the fallback on Windows Home. A full host disk under a growing VHDX risks I/O errors inside WSL.
2. Install the C++ toolchain: `winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive"`. This adds MSVC v143 and the Windows 11 SDK, about 7 GB on C:.
3. Fix PATH: remove `C:\Program Files\Git\usr\bin` (and `mingw64\bin`) from PATH, keeping `Git\cmd`. Alternatively, always build from "Developer PowerShell for VS 2022".
4. Clone onto NTFS at a path without spaces, for example `C:\src\oh-my-gigabytes`, with `CARGO_TARGET_DIR=C:\src\omg-target`. Do not build from `\\wsl.localhost\...`: it is slow and UNC paths break tools.
5. Install PowerShell 7 (`winget install Microsoft.PowerShell`) and keep testing 5.1 as well. Enable Developer Mode so tests can create symlinks without admin.

Acceptance: `cargo fmt --check`, `cargo clippy -D warnings` and `cargo test --workspace` pass natively with the pinned 1.95.0 MSVC toolchain. Record the commands and timings in `docs/windows-evidence.md`.

### W01: Lossless paths

- Store Windows root-relative paths as **WTF-8 bytes with `/` separators**, produced by OMG's own encoder from `encode_wide` (do not persist `OsStr::as_encoded_bytes`; its encoding is unspecified). `/` cannot appear in a Windows name, so the normalisation is lossless. Every existing `b'/'` routine (`policy::inside`, `rollup::parent_of`, `cleanup::descendants`) keeps working.
- Decode back to UTF-16 for every API call, always as a verbatim `\\?\C:\...` path, or relative to a handle (W02). Display with `\`.
- Policy names compare case-insensitively on Windows (`.GIT`, `Node_Modules`); per-directory case sensitivity is reported, not assumed.
- Handle reserved names (`CON`, `NUL`), trailing dots and spaces, lone surrogates, and 8.3 short names (never used as authority).

Acceptance: round-trip property test over arbitrary `u16` sequences; fixtures with each case above.

### W02: Native NTFS scanner

| Concern | Linux today | Windows design |
| --- | --- | --- |
| Root | `openat2`, `fstatfs` magic | `CreateFileW(\\?\root, FILE_FLAG_BACKUP_SEMANTICS \| FILE_FLAG_OPEN_REPARSE_POINT)`; reject reparse roots; filesystem name via `GetVolumeInformationByHandleW`; drive type via `GetDriveTypeW` |
| Children | `openat2(BENEATH \| NO_SYMLINKS \| NO_XDEV)` | `NtCreateFile` relative to the parent handle (`RootDirectory`), one component, `FILE_OPEN_REPARSE_POINT`; verify the opened FileId equals the enumerated one |
| Enumeration | `getdents` plus one `statat` | `GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)` with 64 KiB+ buffers. It returns name, 128-bit FileId, attributes, reparse tag, sizes and times **with no per-entry open** |
| Identity | `dev:ino` | `volumeSerial:fileId128`. NTFS FileIds embed a sequence number, so reuse is detectable |
| Link count | `st_nlink` | Not in enumeration: stored as **unknown** (migration 012 makes `links` nullable). Fetched by handle only for review and delete targets |
| Redirection | symlinks excluded | Any `FILE_ATTRIBUTE_REPARSE_POINT` is not descended and is recorded as excluded with its tag (symlink, junction, mount point, AppExecLink, WCI, dedup) |
| Mounts | `st_dev` change | Mount-point reparse tag or volume-serial change |
| Cloud | path names | `IO_REPARSE_TAG_CLOUD_*`, `RECALL_ON_OPEN`, `RECALL_ON_DATA_ACCESS`, `OFFLINE`: never opened, never descended; `%OneDrive%` added to mandatory exclusions |
| Allocation | `st_blocks*512` | `AllocationSize` (correct for compressed and sparse). MFT-resident small files report 0. Alternate data streams are not counted (documented) |

Refactor `scanner.rs` so the worker queue, batching, writer and rollups are platform-independent. Only "open root / list directory / open child" differ (`platform::linux` and `platform::windows`). Use `windows-sys` (Microsoft, MIT/Apache-2.0) with exact versions. Make `rustix` a Unix-only dependency.

No MFT or USN reading: that needs elevation, and S09 forbids automatic elevation.

Acceptance: a junction loop and a symlink out of scope are excluded; OneDrive "online-only" files keep their attributes after a scan (no hydration); hard links (`mklink /H`) are deduplicated; `\\?\` paths over 260 characters work; totals equal an independent `std::fs` oracle (same test style as `tests/rollup.rs`).

### W03: State directory and locks

- Default state is `%LOCALAPPDATA%\omg`, matching `operations.md`.
- Ownership check: the owner SID equals the current user, with a protected DACL (user plus SYSTEM only), created explicitly. Inherited `Everyone` or `Users` write is rejected. This replaces the `0700` check.
- `index::lock` uses `LockFileEx(EXCLUSIVE | FAIL_IMMEDIATELY)`, mapping `ERROR_LOCK_VIOLATION` to `Busy`. `lock_held` opens only if the file exists. Locks release on process exit, so a crash cannot strand them.
- Measure Defender's effect on SQLite WAL commits. Never advise excluding OMG from Defender.

### W04: Drives and disk numbers

- Volume numbers come from `GetDiskFreeSpaceExW`, reporting both caller-available and total free bytes. Disk use is shown the way Explorer shows it.
- A **Drives** view lists each volume (C:, D:, F:) with filesystem, type, capacity and free space. Totals are never summed across drives. On this machine it would show D: at 0 bytes free.
- `capability::Filesystem` gains `Ntfs`, `Refs` and `Fat`. Deletion support stays per filesystem, as on Linux.

### W05: Windows findings (catalog, info/review tiers only)

- Windows discovery roots: npm `%LOCALAPPDATA%\npm-cache`; pip `%LOCALAPPDATA%\pip\Cache`; uv `%LOCALAPPDATA%\uv\cache`; Cargo `%CARGO_HOME%\registry`; Gradle `%USERPROFILE%\.gradle\caches`; NuGet `%USERPROFILE%\.nuget\packages`; Yarn and pnpm stores; `%TEMP%`; browser and VS Code caches; large installers in Downloads.
- **Virtual disks** (high value here): WSL distro VHDX files found through `HKCU\...\Lxss\*\BasePath`, Docker Desktop's `docker_data.vhdx`. Show file size against used space when it is known (77 GB of 174 GB here), and give the reclaim steps as **text the user runs**. OMG never runs `wsl` or `diskpart` (S08, S09).
- Recycle Bin size per drive via `SHQueryRecycleBinW` (information only).
- Protected, never actionable: `C:\Windows`, `Program Files*`, `ProgramData`, `AppData\Roaming\Microsoft\{Credentials,Protect,Crypto}`, `.ssh`, `pagefile.sys`, `hiberfil.sys`, `System Volume Information`, `$Recycle.Bin` contents, WindowsApps.

### W06: Windows Terminal experience

| Item | Plan |
| --- | --- |
| Colours | Keep `WT_SESSION` truecolor detection. On legacy conhost, use 256 colours and test `NO_COLOR` |
| Glyphs | Verify the block-letter banner and box drawing in Cascadia Mono, plus an ASCII fallback when VT is unavailable |
| Keys | Already `Press`-only. Test AltGr layouts, Shift+Tab, and F-keys in WT and conhost |
| Ctrl+C / closing a tab | Enable `ctrlc`'s `termination` feature on Windows so `CTRL_CLOSE_EVENT` (about 5 s grace) cancels a scan and restores the console |
| Launcher | In `omg.js`, on `win32` do not forward signals: the console already delivers Ctrl+C to the child. Wait for the child's exit code instead |
| Text output | Console output uses `WriteConsoleW` (correct). Piped output in PowerShell 5.1 uses the OEM code page and garbles names, so recommend `--json` for scripting and document `[Console]::OutputEncoding` |
| Manifest | Embed `longPathAware`, `activeCodePage=UTF-8`, supported OS (Windows 10/11), and version info |
| Paths | Show `C:\Users\...` with `\`; sanitise control characters exactly as on Linux |
| Optional | `omg terminal-profile install` writes a Windows Terminal fragment (`%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\omg\omg.json`). Only on explicit request |

Acceptance: a ConPTY smoke test (Windows equivalent of `tests/test_*.py`) drives the TUI, resizes, quits, and checks the console mode is restored. Manual matrix: WT with PowerShell 5.1, WT with cmd, WT with PowerShell 7, and conhost.

### W07: Scan-only background service

- Registration: a per-user Task Scheduler task from fixed XML via `schtasks.exe` with fixed argv. `InteractiveToken`, `LeastPrivilege`, no stored password. Triggers at logon and on an interval. `DisallowStartIfOnBatteries`. Task priority 7, plus `PROCESS_MODE_BACKGROUND_BEGIN` for low I/O priority.
- IPC: a named pipe with `PIPE_REJECT_REMOTE_CLIENTS` and a user-only DACL. Peers are checked by comparing the client's token SID (the `SO_PEERCRED` equivalent). The closed enum `status | scan | yield | stop` is unchanged.
- The marker-line ownership rule from `unit.rs` applies to the task description. Tests use a fake `schtasks` and never touch the real scheduler.
- New service files are added to the structural guard test (`service_logic_contains_no_deletion_or_cleanup_calls`).

### W08: Deletion on NTFS (M5, after DW2)

Windows can close the gap that Linux cannot. A file can be deleted **by handle**:

1. Open the target relative to its verified parent handle with `DELETE` access and a share mode **without** `FILE_SHARE_DELETE`. Nobody can rename or replace it while it is held.
2. Validate that same handle: FileId, volume serial, creation time, owner SID, attributes (not reparse, not cloud, not system), and link count (hard-link accounting from the Linux fix).
3. Delete with `SetFileInformationByHandle(FileDispositionInfoEx, DELETE | POSIX_SEMANTICS)`. Directories are emptied bottom-up through child handles.

Scope mirrors D15: `%USERPROFILE%` only, never OneDrive or cloud, never protected roots. Files in use (sharing violations) are skipped with a reason, and the read-only attribute is never cleared silently.

The Recycle Bin (S07) stays **unsupported**. `IFileOperation` is path-based, and writing `$Recycle.Bin` records directly relies on an undocumented format. Investigate under safety.md §5 before claiming it.

Acceptance: the Linux `tests/cleanup.rs` scenarios ported (replacement, swap to a junction, protected child inserted, hard-link survivors, cancellation, interrupted intent) pass on NTFS.

### W09: Packaging

- `win32-x64` native package with a static CRT (`-C target-feature=+crt-static`), so there is no VC++ redistributable dependency. ARM64 comes later, after native tests.
- `scripts/assemble-release.js`, `lib/verify.js` and `release.test.js` change together for Windows capabilities (CLAUDE.md coupling rule). Windows is declared `cleanup: unsupported` until M5.
- Authenticode signing (DW5) in addition to the Ed25519 manifest.
- PowerShell execution policy: npm's `omg.ps1` shim is blocked where the policy is `Restricted`. Document `omg.cmd` and the policy, and consider a WinGet manifest for a direct `omg.exe`.

### W10: Performance

- Baselines on this machine: entries per second for `C:\Users\SK AKRAM` (NTFS) against the same tree in WSL ext4 (about 50,000 entries/s recorded in `phase1-evidence.md`), plus TUI first-frame time and launcher overhead (Node startup plus hashing the binary on every launch).
- Levers: large enumeration buffers, parallel directory workers (existing queue), no per-entry opens, and batched SQLite writes (existing).
- Report measured numbers only. No performance claims without native evidence.

### W11: CI and tests

- Remove the Linux-only `#![cfg]` from integration tests as each Windows adapter lands. Tests needing symlinks skip with a printed reason when Developer Mode or privilege is absent.
- Fixture creation on Windows: `mklink /J` (no admin), `mklink /H`, an OneDrive-style placeholder (only if a test sync root can be registered; otherwise record the gap), long paths, and odd names.
- Add the ConPTY smoke test to the `windows-latest` leg. Keep "stub compile is not certification" until the evidence exists.

## 5. Evidence and documentation

- New `docs/windows-evidence.md`: environment, exact commands, measured results, gates not passed (same rules as `phaseN-evidence.md`).
- On completion, update `checklist.md`, `release.md`, `stack.md` (Windows row) and the `plans.md` decision record (DW1–DW5).
- Never claim Windows certification, performance or "safe to delete" without native evidence on this machine or CI.

## 6. Risks

| Risk | Mitigation |
| --- | --- |
| Hydrating cloud files | Attribute and reparse checks before any open; mandatory OneDrive exclusion; attribute-preservation test |
| Path authority lost to display strings | WTF-8 storage plus verbatim paths (W01) |
| Antivirus slowing state writes | Measure; batch commits; never weaken Defender |
| Space in user name and tool PATH conflicts | Quote everything; W00 environment fixes |
| Full host disk under the WSL VHDX | W00 step 1 now; W05 finding afterwards |
| Scope creep into a Windows "cleaner" | Info/review tiers only; deletion only at M5 behind DW2 |
