# oh-my-gigabytes

OMG (Oh My Gigabytes) shows where your disk space went. It is a local-first, keyboard-first storage analyzer for developers: a Rust engine scans file metadata into a local SQLite index, and a terminal interface lets you browse folder sizes and see which known developer caches (npm, pip, uv, pnpm, Cargo, Go, Gradle, Homebrew) take space and why.

> **Beta (`0.1.0-beta.1`): analysis only.** The released build cannot delete, move or change any of your files. It reads metadata (names, sizes, times), never file contents, and keeps its index in a private folder on your machine. Nothing is uploaded.

## What works where

| System | Versions | Filesystems scanned | Notes |
| --- | --- | --- | --- |
| Linux x86_64 and ARM64 (glibc) | glibc 2.28 or newer **and a kernel 5.6 or newer** (for example Ubuntu 22.04+, Debian 11+, RHEL/Alma 9+, Fedora, Amazon Linux 2023; not RHEL 8 or the Ubuntu 20.04 base kernel) | ext4/ext3/ext2, XFS, Btrfs, tmpfs | Not musl (Alpine). Overlayfs, ZFS, F2FS, NFS and FUSE are refused. Optional background scan service (systemd user unit). |
| Windows x64 | Windows 11 and Windows Server 2022/2025 tested; Windows 10 untested | NTFS, ReFS | FAT32 and exFAT drives are refused with a message. No background service. The binary needs no Visual C++ redistributable. |
| macOS arm64 and Intel | macOS 15 or newer | APFS, HFS+, exFAT, FAT | No background service. |

How each platform was tested is recorded in [Linux](docs/linux-qualification.md), [Windows](docs/windows-evidence.md) and [macOS](docs/macos-evidence.md) evidence pages. In short: Linux x86_64 was tested on WSL2 and on hosted runners (Linux ARM64 and the glibc 2.28 floor on hosted runners and in distro containers), Windows on hosted runners and a Windows 11 PC, and macOS on hosted runners only. **No platform is certified.** macOS has never run on a real Mac; privacy prompts (Full Disk Access), iCloud placeholder files and Gatekeeper dialogs are untested.

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

Use `--state-dir <folder>` to keep a separate index. Give `scan --root` the real path of a folder: on macOS `/tmp` and `/var` are links, so use `/private/tmp`. Hard-linked files and links are counted without being followed. Sizes are what the filesystem reports; on Btrfs, APFS and similar, shared blocks mean that deleting something may free less than shown.

## Not in this release

Deleting files, moving files to the system trash, a macOS or Windows background service, signed Windows executables (Authenticode), Apple notarization, Windows on Arm and musl builds. The source tree contains an unqualified, explicitly confirmed permanent-delete feature for Linux and Windows development builds; it is compiled out of the beta and is not recommended for use.

## Security

Report problems privately as described in [SECURITY.md](SECURITY.md). The index, configuration and background service are local only; the service speaks to nothing but your own account over a Unix socket and cannot touch files.

## Develop

Rust 1.95.0 (pinned). See [CLAUDE.md](CLAUDE.md) for commands and conventions, [docs/beta-release-progress.md](docs/beta-release-progress.md) for release status, [docs/release.md](docs/release.md) for how a release is built and signed, and [docs/README.md](docs/README.md) for the product brief, safety invariants and phase evidence.

## License

MIT OR Apache-2.0. The rule catalog is independently authored and under the same terms.
