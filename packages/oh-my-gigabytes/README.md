<p align="center">
  <img src="https://raw.githubusercontent.com/akramcodez/oh-my-gigabytes/main/.github/banner.png" alt="OMG (Oh My Gigabytes): see where your disk went. npm i -g oh-my-gigabytes" width="100%">
</p>

<p align="center">
  <a href="https://www.oh-my-gigabytes.com"><b>Website</b></a> ·
  <a href="https://github.com/akramcodez/oh-my-gigabytes">GitHub</a> ·
  <a href="https://github.com/akramcodez/oh-my-gigabytes/releases">Releases</a> ·
  <a href="https://github.com/akramcodez/oh-my-gigabytes/blob/main/CHANGELOG.md">Changelog</a> ·
  <a href="https://github.com/akramcodez/oh-my-gigabytes/blob/main/docs/safety.md">Safety rules</a>
</p>

**OMG (Oh My Gigabytes)** shows where your disk space went and helps you take it back: developer caches, build folders, old toolchains and leftovers, each with an explanation, deleted only after you confirm. Everything runs on your machine; nothing is uploaded.

> **Development preview.** Delete is permanent and not qualified on any platform.

## Install

```sh
npm install --global oh-my-gigabytes
omg
```

Needs Node 22+. Linux x86_64 and ARM64 (glibc 2.28+, kernel 5.6+), Windows 11 x64, macOS 15+ on Apple silicon.

The first launch scans your home folder (names, sizes and times only). On macOS, allow your terminal app in System Settings › Privacy & Security if it asks about Desktop, Documents or Downloads.

## Features

- **Finds developer clutter:** package caches, build output, old toolchain and IDE versions, AI tool sessions, downloaded models, Docker leftovers, old installers and large files.
- **Explains every finding:** what it is, why it can go, and when it last changed.
- **You decide:** suggestions wait in OMG Trash, a saved list. Nothing is deleted until you confirm, and Cancel is the default.
- **Refuses the dangerous parts:** home folder only; keys, credentials, databases, `.git`, cloud-synced folders and active repositories are never deleted, and links are never followed.

## Keys

| Key | Does |
| --- | --- |
| `1` to `5` | Overview, Browse, Findings, OMG Trash, Deleted |
| `↑` `↓` or `j` `k` | Move; `⏎` opens or shows details, `⌫` goes back |
| `␣` | In Browse or Findings, add to OMG Trash (nothing moves) |
| `u` / `U` | Remove the selected item / everything from OMG Trash |
| `d` / `D` | Delete the selected item / everything in OMG Trash, after a confirmation |
| `r` | Scan again |
| `m` | Options: auto delete, Docker cleanup, suggestions, theme |
| `?` / `q` | All shortcuts / quit |

## Clean up

1. After a scan, **OMG Trash** (`4`) lists suggestions: package caches, build output, old tool and extension versions, untouched Git clones, and app data or toolchains unused for 5 months.
2. Review the list. `u` takes out anything you want to keep; in Browse or Findings, `␣` adds more.
3. `d` deletes the selected item, `D` everything listed. "Are you sure?" shows the exact paths and size.
4. Choose **"Yes, delete permanently"**. There is no undo. Each item is re-checked just before it is removed.

**Auto delete** is off by default. Turn it on from `m` to delete, in the background, what has stayed in OMG Trash unchanged for 1 day, 7 days, 1 month or 6 months.

## Commands

```sh
omg scan --root ~/code                   # scan without opening the interface
omg findings                             # findings as JSON
omg status                               # totals, and how much space omg itself uses
omg grew                                 # which folders grew since the scan before
omg clean --category logs_and_temp       # preview a category; add --confirm <fingerprint> to delete it
omg docker clean                         # unused Docker data, removed by Docker itself
omg doctor                               # read-only health check
```

## Uninstall

```sh
omg auto-delete off
omg schedule disable        # macOS, Windows
omg service uninstall       # Linux
npm uninstall --global oh-my-gigabytes
```

## Safety

No install scripts and no downloads. Every launch verifies the binary against an Ed25519-signed manifest. Release key fingerprint: `01deacfe588025e045419b57c4f39abd9f151294c8310e0a0dd28a18c9d9adc0`.

Security reports: [SECURITY.md](https://github.com/akramcodez/oh-my-gigabytes/blob/main/SECURITY.md) · MIT OR Apache-2.0
