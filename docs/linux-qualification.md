# Linux qualification matrix

Status: **no profile is qualified.** This page records what was actually run, and what is still missing. Updated 2026-10-15. A profile moves to `qualified` only when every capability's native evidence below is recorded and reproducible. Until then `capability::permanent_delete` reports `development-unqualified` for ext-family, tmpfs and Btrfs, and `os_trash` is `unsupported`.

## Capabilities

| Capability | Meaning | Current status |
| --- | --- | --- |
| Analysis | Metadata scan, index, TUI/CLI browsing, findings | Development; tested on tmpfs and Btrfs |
| Permanent delete (D15) | Home-only, confirmed plan, revalidated, not recoverable | `development-unqualified` on ext-family, tmpfs, Btrfs |
| OS Trash | Reversible move to the freedesktop Trash | `unsupported` (see `linux-os-trash-investigation.md`) |
| Service | Optional scan-only systemd user unit | Development; Linux-only tests pass here; desktop notifications broken on this host |

## Profiles

| Profile | Host | Analysis | Permanent delete | Birth time | Result |
| --- | --- | --- | --- | --- | --- |
| Fedora 44, kernel 7.2.9-201, x86_64, Btrfs `/home` | Owner's desktop | Scan and CLI tests passed (native, prior session; ignored Btrfs tests not re-run after cleanup in this session) | Unit and integration tests run on tmpfs; not run on this Btrfs home | Reported by `stat -c %w` on 2026-10-15 | Development only |
| tmpfs (`/tmp`) on the same host | Same | Full test suite passed (2026-10-15) | Full cleanup suite passed (16 tests, including race barriers and hard-link oracle) | Reported | Development only |
| ext4 (loop image, `scripts/ext4-sandbox.sh`, kernel 7.2.9) | Owner's host, 2026-10-15 | Scanner suite: 9 passed, 1 ignored (Btrfs-only) | Cleanup suite: 16 passed (race barriers, hard-link oracle, partial removal, interrupted attempt) | Not captured in that run; `up` now prints it | Development: passed on ext4 |
| WSL2 ext4 | **Not available** | Not run | Not run | Not measured | **Missing** |
| Linux ARM64 GNU | **Not available** | Not built | Not run | — | **Missing** |
| Oldest supported kernel (openat2) | **Not available** | Not run | Not run | — | **Missing** |
| FUSE, cloud, network mounts (sshfs, rclone, gvfs) | Not tested | Expected to be refused by root acquisition (filesystem allowlist); not verified | Not run | — | **Missing** |

## Requirements observed (not yet proven minimums)

- `openat2` with `RESOLVE_NO_SYMLINKS | BENEATH | NO_XDEV` is used for every root and descendant walk. The oldest kernel that provides it has not been tested.
- The state directory must be owned by the current user with mode 0700 (checked on every open).
- glibc build from `cargo build --locked`. The minimum glibc baseline is not measured.
- Filesystem families accepted: ext2/3/4 (`0xef53`), tmpfs, Btrfs. All others are refused at root acquisition.

## Known limits in this matrix

- Birth time is reported on tmpfs and Btrfs here. Where birth time is missing, revalidation falls back to identity only, which is documented as a residual risk (WP05).
- Btrfs shared extents and snapshots mean freed space can be smaller than the indexed size. The UI and docs say so; Btrfs deletion is not qualified.
- Service: `notify-send` on this host fails with a libnotify symbol error (`undefined symbol: notify_notification_get_activation_app_launch_context`). The D-Bus notification server itself accepted a test message, so the failure is in the local `notify-send` binary. Service desktop notifications are therefore **not verified**.
- WSL2 virtual disks: reclaiming space after deletion is an explanatory note only. OMG never compacts disks.

## What would move a profile to qualified

1. Native ext4 host: full test suite, the cleanup race suite and the ignored Btrfs-style suites run against an ext4 root, with results recorded.
2. Each enabled capability's race and failure tests (WP05) passing on that profile.
3. A reviewed constant or table entry that maps the profile to `qualified`, tied to this document.
4. The release manifest and `omg doctor` updated in the same change (`assemble-release.js`, `verify.js`, `release.test.js`).
