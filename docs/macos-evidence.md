# macOS evidence

Status: **no native macOS adapter yet.** macOS compiles with the native adapters disabled. This page records what was measured on GitHub-hosted macOS runners so the adapter (work package R04) is built on facts. The owner has no Mac, so the hosted runners are the only test bed. Nothing here is a certification. Plan: `docs/checklist.md`, release plan R03-R06.

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

## Gates not passed

No adapter, no scanner test on macOS, no PTY test on macOS, no packaged install on macOS. The checklist items for macOS stay unchecked.
