# oh-my-gigabytes

OMG (Oh My Gigabytes) shows where your disk space went and helps you take it back. A Rust engine scans file metadata into a local index, and a terminal interface lets you browse folder sizes, see which developer caches and build folders take space and why, and delete what you choose.

> **Beta (`0.1.0-beta.2`), a development preview.** Scanning reads names, sizes and times, not the contents of your files. The one exception: to tell whether an old Git clone has nothing local to lose, it reads Git's own config and branch references. Nothing is uploaded. Delete is permanent and not qualified: read [Deleting files](#deleting-files) first.

## Platforms

| System | Scans | Deletes on | Notes |
| --- | --- | --- | --- |
| Linux x86_64, ARM64 (glibc 2.28+, kernel 5.6+) | ext4, XFS, Btrfs, tmpfs | ext4, Btrfs, tmpfs | Not musl. Optional background scan service. |
| Windows 11 x64 | NTFS, ReFS | NTFS | FAT32 and exFAT are refused. |
| macOS 15+ on Apple silicon | APFS, HFS+, exFAT, FAT | APFS | No Intel Macs. |

No platform is certified. Test records: [Linux](docs/linux-qualification.md), [macOS](docs/macos-evidence.md), [Windows](docs/windows-evidence.md), [shared](docs/evidence.md).

## Install

Needs Node 22 or newer.

```sh
npm install --global oh-my-gigabytes@beta
```

The package runs no install scripts and downloads nothing; each launch re-verifies the binary against an Ed25519-signed manifest. To install a downloaded release offline, check `SHA256SUMS` (`shasum -a 256 -c SHA256SUMS` on macOS), compare `KEY-FINGERPRINT.txt` with the fingerprint published in the release notes, then run `npm install --global --ignore-scripts --offline ./oh-my-gigabytes-offline-<target>-0.1.0-beta.2.tgz`.

## Use

```sh
omg scan --root "$HOME" --accept-gaps   # index your home folder
omg                                      # interface: 1 Overview, 2 Browse, 3 Findings, 4 OMG Trash, q quits
omg status                               # totals, and how much space omg itself uses (index_bytes)
omg findings                             # what can be cleaned, with explanations
omg doctor                               # read-only health check
```

Folders omg cannot read show as `unknown`, never as empty. On macOS, allow your terminal app in System Settings › Privacy & Security to scan Desktop, Documents and Downloads.

## Deleting files

- `␣` adds a path to **OMG Trash**, a saved list; nothing moves. `d` deletes the selected item and `D` everything listed, after an "Are you sure?" whose default is Cancel. There is no undo.
- After a scan, OMG Trash suggests: package caches; build output and dependencies of 50 MB or more (`target` next to `Cargo.toml`, `node_modules` next to `package.json`, Python virtualenvs); app caches of 50 MB or more in `~/Library/Caches`; old versions of an AI coding tool; its session history older than 7 days; Git clones untouched for 30 days whose branches all match the remote, with no local commits, stashes or changes; application data folders of 50 MB or more in which nothing changed for 5 months (`AppData` on Windows, `~/Library/Application Support` on macOS, `~/.local/share`, `~/.config` and `~/.cache` on Linux), never one with a database or key inside; and toolchains (rustup, nvm, pyenv, SDKMAN!, the Android SDK) unchanged for 5 months with no build of that language in that time. Turn suggestions off from the options menu.
- A whole project, session history or application's data is deleted only on its own, never with `D`.
- Home folder only. Credentials, keys, databases, disk images, `.git`, `.ssh`, cloud-synced folders and active Git repositories are refused, and links are never followed. Container and VM disks (`.colima`, Docker Desktop) are shown but never deleted.
- **Docker:** press `m` and choose **Clean unused Docker data**, or run `omg docker clean`. omg asks Docker itself, on this machine only, for dangling images older than 7 days and build cache unused for 30 days, and after you confirm, Docker removes exactly those, one by one and without force. `--include-unused-images` and `--include-stopped-containers` add tagged images and stopped containers older than 30 days. Volumes, running containers and anything in use are never touched.
- On a Mac, iCloud placeholders, locked files, app bundles and folders such as `~/Library` itself are refused, and deleting inside Desktop or Documents warns that iCloud may remove the file everywhere.
- Before removing anything, omg re-checks that each item is still the same file, and skips it if not.
- Freed space can be less than shown: hard links, APFS clones and snapshots share data.

## If the index is damaged

omg refuses to use an index it cannot read and changes nothing. Move its state folder aside (`~/Library/Application Support/omg` on macOS, `~/.local/state/omg` on Linux, `%LOCALAPPDATA%\omg` on Windows, or `--state-dir`) and scan again.

## Not in this release

Moving to the system trash, a macOS or Windows background service, Authenticode signing, Apple notarization, Intel Macs, Windows on Arm and musl.

## More

Security reports: [SECURITY.md](SECURITY.md). Safety rules: [docs/safety.md](docs/safety.md). Building and signing a release: [docs/release.md](docs/release.md). Development: Rust 1.95.0, commands in [CLAUDE.md](CLAUDE.md).

MIT OR Apache-2.0. The rule catalog is independently authored and under the same terms.
