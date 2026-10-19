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
| WSL2 ext4 (Ubuntu 24.04.3, kernel 5.15.167.4, glibc 2.39, x86_64; `/` and `/tmp` are ext4) | Owner's PC, 2026-10-18 | Full workspace suite passed: scanner 9, rollup 5, policy 3, planning 1, CLI 10, core unit 16, migrations 4, service 20 | Cleanup suite 19 passed in the default build; the analysis-only build (the one released) passed too | Not recorded | Development: passed on WSL2 ext4. Same host, kernel 5.15 also confirms `openat2` works on 5.15 |
| Linux ARM64 GNU (GitHub `ubuntu-24.04-arm`, Ubuntu 24.04, aarch64) | Hosted runner, 2026-10-18 and 2026-10-19 | Full workspace suite passed in the default and analysis-only builds (run 39111223427); a `st_nlink` type error on aarch64 was found and fixed | Cleanup suite passed in the default build | Not recorded | Development: hosted-runner evidence only |
| Ubuntu 22.04 (GitHub `ubuntu-22.04`, glibc 2.35, x86_64) | Hosted runner | Full suite passed | Passed | Not recorded | Development: hosted-runner evidence only |
| Distro userlands in containers (`ubuntu:20.04`, `22.04`, `24.04`, `debian:11`, `12`, `almalinux:8`, `9`, `fedora:latest`, `amazonlinux:2023`) | Owner's PC, Docker 29.1.3, 2026-10-18 | The glibc 2.28 release binary ran `--version`, `scan`, `status` and `doctor` on a bind-mounted ext4 folder in each | Not run | — | Passed. All containers share the WSL kernel 5.15, so this tests the C library and userland, not kernels. `alpine:3.20` cannot run the binary (no glibc), as intended |
| glibc floor | Release build, 2026-10-19 (run 39111562168) | Built in `manylinux_2_28` for x86_64 and aarch64; `scripts/check-glibc.sh` found a maximum of `GLIBC_2.28` in both | — | — | Passed |
| Oldest supported kernel (openat2, Linux 5.6) | **Not available** | Not run | Not run | — | **Missing**. Lowest kernel observed: 5.15. The error for a kernel without `openat2` is unit-tested only for its wording, not exercised on such a kernel |
| Service: desktop delivery contract | Owner's PC, 2026-10-19 | `notify::deliver_with` run against a stand-in `notify-send` script: argument list is exactly `--app-name=omg`, `--`, the title and the text; text with shell metacharacters is not executed; a missing program is reported, not fatal | — | — | Passed. Delivery to a real desktop is **not** verified: the owner's `notify-send` has a library error and WSL has no notification daemon |
| Service: second account | Owner's PC, Docker (root in `ubuntu:24.04`, client as `nobody`), 2026-10-19 | A connection from another user to a deliberately world-connectable socket got no answer, `rejected_peers` was 1 and the request was never handled (`a_connection_from_another_user_is_dropped_without_an_answer`); also runs in CI on `ubuntu-24.04` with `sudo -u omgother` | — | — | Passed |
| Kernel without `openat2` (simulated) | Owner's PC, 2026-10-19 | `scripts/check-no-openat2.sh`: with `openat2` returning `ENOSYS` through a seccomp filter, `omg scan` stops with "openat2 is unavailable: omg needs Linux 5.6 or newer" and creates no database | — | — | Passed as a simulation on kernel 5.15. A real kernel older than 5.6 is still untested |
| XFS (loop image) | **Not available** | Not run: `mkfs.xfs` is not installed and `sudo` needs a password on the test host | Refused by design | — | **Missing**. XFS is accepted for scanning in code and the magic number is unit-tested; `scripts/fs-sandbox.sh xfs` is untried |
| FUSE, cloud, network mounts (sshfs, rclone, gvfs) | Not tested | Expected to be refused by root acquisition (filesystem allowlist); not verified | Not run | — | **Missing** |

## Requirements observed (not yet proven minimums)

- `openat2` with `RESOLVE_NO_SYMLINKS | BENEATH | NO_XDEV` is used for every root and descendant walk. The oldest kernel that provides it has not been tested.
- The state directory must be owned by the current user with mode 0700 (checked on every open).
- Release binaries are built in a glibc 2.28 container and need glibc 2.28 or newer (measured, see above). A development build made on a newer distro needs that distro's glibc.
- Filesystem families accepted for scanning: ext2/3/4 (`0xef53`), XFS (`0x58465342`), tmpfs, Btrfs. All others are refused at root acquisition. Permanent delete is never offered on XFS.

## Known limits in this matrix

- Birth time is reported on tmpfs and Btrfs here, and by `stat -c %w` on the ext4 root of this WSL2 host (2026-10-15). Where birth time is missing, OMG refuses to save or delete the item; it no longer falls back to identity alone (WP05).
- Btrfs shared extents and snapshots mean freed space can be smaller than the indexed size. The UI and docs say so; Btrfs deletion is not qualified.
- Service: `notify-send` on this host fails with a libnotify symbol error (`undefined symbol: notify_notification_get_activation_app_launch_context`). The D-Bus notification server itself accepted a test message, so the failure is in the local `notify-send` binary. Service desktop notifications are therefore **not verified**.
- WSL2 virtual disks: reclaiming space after deletion is an explanatory note only. OMG never compacts disks.

## What would move a profile to qualified

1. Native ext4 host: full test suite, the cleanup race suite and the ignored Btrfs-style suites run against an ext4 root, with results recorded.
2. Each enabled capability's race and failure tests (WP05) passing on that profile.
3. A reviewed constant or table entry that maps the profile to `qualified`, tied to this document.
4. The release manifest and `omg doctor` updated in the same change (`assemble-release.js`, `verify.js`, `release.test.js`).
