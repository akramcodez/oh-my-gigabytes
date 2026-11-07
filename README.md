# oh-my-gigabytes

OMG (Oh My Gigabytes) shows where your disk space went. It is a local-first, keyboard-first storage analyzer for developers: a Rust engine scans file metadata into a local SQLite index, and a terminal interface lets you browse folder sizes and see which known developer caches (npm, pip, uv, pnpm, Cargo, Go, Gradle, Homebrew) take space and why.

> **Beta (`0.1.0-beta.1`), a development preview.** Scanning reads metadata (names, sizes, times), never file contents, and keeps its index in a private folder on your machine. Nothing is uploaded. **The build can also permanently delete files you choose**, only after you confirm an exact list, and that feature is not qualified: read [Deleting files](#deleting-files) first.

## What works where

| System | Versions | Filesystems scanned | Permanent delete | Notes |
| --- | --- | --- | --- | --- |
| Linux x86_64 and ARM64 (glibc) | glibc 2.28 or newer **and a kernel 5.6 or newer** (for example Ubuntu 22.04+, Debian 11+, RHEL/Alma 9+, Fedora, Amazon Linux 2023; not RHEL 8 or the Ubuntu 20.04 base kernel) | ext4/ext3/ext2, XFS, Btrfs, tmpfs | ext4, Btrfs and tmpfs (not XFS) | Not musl (Alpine). Overlayfs, ZFS, F2FS, NFS and FUSE are refused. Optional background scan service (systemd user unit). |
| Windows 11 x64 | Windows 11 only; other Windows versions, including Windows 10, are not targets | NTFS, ReFS | NTFS only (not ReFS) | FAT32 and exFAT drives are refused with a message. No background service. The binary needs no Visual C++ redistributable. |
| macOS on Apple silicon (arm64) | macOS 15 or newer | APFS, HFS+, exFAT, FAT | APFS only | Intel Macs are not supported in this release. No background service. |

How each platform was tested is recorded in [Linux](docs/linux-qualification.md), [Windows](docs/windows-evidence.md) and [macOS](docs/macos-evidence.md) evidence pages. In short: Linux x86_64 was tested on WSL2 and on hosted runners (Linux ARM64 and the glibc 2.28 floor on hosted runners and in distro containers), Windows on hosted runners and a Windows 11 PC, and macOS on hosted runners and one Apple silicon Mac (macOS 26). **No platform is certified.** On macOS, Full Disk Access and iCloud placeholder files are untested on real hardware.

## Install

Needs Node 22 or newer. Until the beta is published, install a release you downloaded and verified (see below). Once it is on npm:

```sh
npm install --global oh-my-gigabytes@beta
omg --version
```

The package contains no install scripts and downloads nothing. Each launch re-verifies the platform binary against an Ed25519-signed manifest.

Offline or manual install from a release: download the files for your system plus `SHA256SUMS`, `release-public.pem` and `KEY-FINGERPRINT.txt`, then

```sh
sha256sum -c SHA256SUMS          # macOS: shasum -a 256 -c SHA256SUMS
npm install --global --ignore-scripts --offline ./oh-my-gigabytes-offline-<target>-0.1.0-beta.1.tgz
```

Compare the fingerprint in `KEY-FINGERPRINT.txt` with the one published separately in the release notes before trusting it. On macOS, a binary downloaded in a browser is not notarized and carries a quarantine mark (`xattr -d com.apple.quarantine <file>` clears it); installing through npm avoids this. Windows may show a SmartScreen warning for a manually downloaded `omg.exe` because it is not Authenticode-signed.

## Use

```sh
omg scan --root ~/Projects     # index a folder (metadata only)
omg                            # open the terminal interface (1-4 switch views, q quits)
omg status                     # what the last scan found
omg tree [PATH]                # sizes of folders under the scan root
omg findings                   # known caches and large files, with explanations
omg doctor                     # read-only health check; also says what this build can do
```

Use `--state-dir <folder>` to keep a separate index. Give `scan --root` the real path of a folder: on macOS `/tmp` and `/var` are links, so use `/private/tmp`. Folders omg cannot read are shown as `unknown` (or their size as `≥ N`), never as empty; on macOS, privacy protection blocks Desktop, Documents, Downloads and parts of `~/Library` until you allow your terminal app in System Settings › Privacy & Security. Hard-linked files and links are counted without being followed. Sizes are what the filesystem reports; on Btrfs, APFS and similar, shared blocks mean that deleting something may free less than shown.

## Deleting files

All builds include permanent delete. `omg doctor` and the release manifest label it `development-unqualified`: it passes the automated tests on hosted runners (Linux x64 and ARM64, Windows), a Windows 11 PC and one Apple silicon Mac, but no platform has been qualified by native evidence, and OMG does not claim that deleting is safe. There is no typed delete command: deleting happens in the interface, after you confirm.

How it works:

- In Browse (2) or Findings (3), `␣` adds a path to **OMG Trash**, a saved list: nothing moves. `d` on a row, or `d` / `D` in OMG Trash (4), opens "Are you sure?" with the exact paths and size. **Cancel is the default**; "Yes, delete permanently" is a separate choice.
- **Deletion is permanent.** There is no Recycle Bin or trash to restore from. On Btrfs and APFS, shared blocks mean freed space can be less than shown; on a Mac, APFS clones and local Time Machine snapshots can keep the space in use after a delete.
- Home folder only. Credentials, keys, databases and disk images, `.git`, `.ssh` and similar folders, and cloud-synced folders are refused, and so is an active Git repository (build and dependency folders in a repository, and stale repositories, get a warning). Links are never followed.
- A cache or build folder with protected items inside (for example a Python package cache, whose libraries include folders called `credentials`) cannot be deleted whole. OMG says so **before** you confirm, and offers "Delete everything except the protected items": those items and the folders leading to them stay, everything else is deleted, after a second confirmation that lists what is kept. Ordinary folders with protected items inside are refused, with no such option.
- Just before removing, OMG re-checks that each item is still the same file (identity and creation time) and skips anything that changed or, on Windows, that another program holds open.
- Where: Linux on ext4, Btrfs and tmpfs (XFS and other filesystems are scan-only); Windows on NTFS only (ReFS, FAT32 and exFAT are refused); macOS on APFS only (HFS+, exFAT and FAT are scan-only).
- On a Mac, OMG also refuses iCloud placeholder files (their data is only in the cloud), locked items (`uchg`) and anything inside them, app bundles (uninstall those from Finder), and standard folders such as `~/Desktop`, `~/Documents` and `~/Library` themselves. Inside Desktop and Documents it warns first: if those folders sync with iCloud, a delete also removes the file from iCloud and your other devices. Folders your terminal is not allowed to read are skipped.
- After a scan, OMG Trash lists suggestions, which you review and confirm: package caches; build output and dependencies of 50 MB or more (`target` next to `Cargo.toml`, `node_modules` next to `package.json`, Python virtual environments); application caches of 50 MB or more in `~/Library/Caches` on a Mac (not Apple's own); old versions of an AI coding tool, keeping the newest and the one in use; and its session transcripts 7 or more days old and 1 MB or larger, which cannot be recreated. Large files are never suggested. Turn suggestions off from the options menu.
- Container and virtual machine disks (`.colima`, `.lima`, `.orbstack`, Docker Desktop) are shown in Findings but cannot be deleted from OMG: one disk holds every image, container and volume. Use `docker system prune` instead.
- Check your build with `omg doctor`: it prints `permanent delete: development-unqualified` when delete is present.

The background service can only scan; it cannot delete.

## Not in this release

Moving files to the system trash or Recycle Bin (delete is permanent), a macOS or Windows background service, signed Windows executables (Authenticode), Apple notarization, Intel Macs, Windows on Arm and musl builds.

## Security

Report problems privately as described in [SECURITY.md](SECURITY.md). The index, configuration and background service are local only; the service speaks to nothing but your own account over a Unix socket and cannot touch files.

## Develop

Rust 1.95.0 (pinned). See [CLAUDE.md](CLAUDE.md) for commands and conventions, [docs/beta-release-progress.md](docs/beta-release-progress.md) for release status, [docs/release.md](docs/release.md) for how a release is built and signed, and [docs/README.md](docs/README.md) for the product brief, safety invariants and phase evidence.

## License

MIT OR Apache-2.0. The rule catalog is independently authored and under the same terms.
