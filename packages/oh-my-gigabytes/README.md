# oh-my-gigabytes

**OMG (Oh My Gigabytes)** shows where your disk space went and helps you take it back: developer caches, build folders, old toolchains and leftovers, each with an explanation, deleted only after you confirm.

> Development-preview beta. Delete is permanent and not qualified. Nothing is uploaded.

## 1. Install

```sh
npm install --global oh-my-gigabytes@beta
omg --version
```

Needs Node 22+. Linux x86_64/ARM64 (glibc 2.28+, kernel 5.6+), Windows 11 x64, macOS 15+ on Apple silicon.

## 2. Open it

```sh
omg
```

The first time, omg scans your home folder (names, sizes and times only). On macOS, allow your terminal app in System Settings › Privacy & Security if it asks about Desktop, Documents or Downloads.

## 3. Look around

| Key | View |
| --- | --- |
| `1` | **Overview**: disk use and the biggest folders |
| `2` | **Browse**: folders by size; `⏎` opens, `⌫` goes back |
| `3` | **Findings**: what can be cleaned and why; `⏎` shows details |
| `4` | **OMG Trash**: what you or the scan picked for deleting |

Move with `↑` `↓` (or `j` `k`). Press `r` to scan again, `?` for all keys, `m` for options, `q` to quit.

## 4. Clean up

1. After a scan, **OMG Trash** (`4`) already lists safe suggestions: package caches, build output (`target`, `node_modules`), app caches, old tool versions, untouched Git clones, and app data or toolchains unused for 5 months.
2. Review the list. `u` removes anything you want to keep; in Browse or Findings, `␣` adds more.
3. `d` deletes the selected item, `D` everything listed. "Are you sure?" shows the exact paths and size, and **Cancel is the default**.
4. Choose **"Yes, delete permanently"**. There is no undo.

Whole projects, session history and app data are deleted only one at a time, never with `D`.

## What omg never deletes

Anything outside your home folder; credentials, keys, databases and disk images; `.git`, `.ssh` and cloud-synced folders; active Git repositories; container and VM disks (use `docker system prune`); and on macOS, iCloud placeholders, locked files and app bundles. Links are never followed, and every item is re-checked just before it is removed.

## Commands

```sh
omg scan --root "$HOME" --accept-gaps   # scan without opening the interface
omg findings                             # findings as JSON
omg status                               # totals, and how much space omg itself uses
omg doctor                               # read-only health check
```

## Safety

No install scripts and no downloads. Every launch verifies the binary against an Ed25519-signed manifest. Release key fingerprint: `01deacfe588025e045419b57c4f39abd9f151294c8310e0a0dd28a18c9d9adc0`.

Source, docs and security reports: https://github.com/akramcodez/oh-my-gigabytes · MIT OR Apache-2.0
