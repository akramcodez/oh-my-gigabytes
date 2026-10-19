# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

OMG (Oh My Gigabytes) is a local-first, keyboard-first storage analyzer for developers. A Rust engine scans filesystem metadata into SQLite, a versioned catalog explains possible cleanup candidates, and an independent policy engine decides what may ever be acted on. Distribution is the npm package `oh-my-gigabytes`. The executable is always `omg`.

It is a **development preview**:
- **Cleanup is explicit and home-scoped (decision D15).** Users add paths to "OMG Trash" (a saved list; nothing moves) and can delete permanently after a confirmed plan, via `cleanup.rs`. Permanent delete runs on Linux and on Windows NTFS. The older `planning::execute` still always fails closed.
- **Linux, Windows and macOS.** Native scanning and state work on all three. Permanent delete exists on Linux and Windows NTFS in the default build only; macOS is analysis only. Windows scans NTFS and ReFS (FAT32/exFAT are refused with a message). The background service is Linux only. macOS scans APFS, HFS+, exFAT and FAT through `platform::macos` (macOS 15 or newer) and has been tested only on GitHub-hosted runners. Status and gaps: `docs/windows-evidence.md`, `docs/macos-evidence.md`, `docs/linux-qualification.md`.
- **First release is an analysis-only beta (`0.1.0-beta.1`).** Release builds use `--no-default-features`, which compiles permanent delete out. Progress and what is left: `docs/beta-release-progress.md`. Procedure: `docs/release.md`, "Releasing the beta".
- **Phases ship as development milestones.** Phases 0–5 are recorded as milestones with unpassed acceptance gates. See `docs/checklist.md`.

## Source-of-truth documents

- `docs/fullcontext.md` is the authoritative product brief.
- `docs/safety.md` defines the mandatory invariants **S01–S12**. No flag, config, catalog rule or user preference may weaken them.
- `docs/checklist.md` and `docs/phaseN-evidence.md` record what is actually implemented and verified, plus the gates that have not passed. Their stated limits override the forward-looking design docs: `architecture.md`, `operations.md`, `ux.md` and `rules.md`.
- When documents disagree, keep the stronger safety requirement.

Documentation is evidence-based:
- A checklist item is `[x]` only when the deliverable exists and its check passed.
- Completing work means updating the matching `docs/phaseN-evidence.md`. Record the environment, the exact verification commands, measured results and the gates that have not passed.
- Never claim platform certification, performance, or "safe to delete" without native evidence.

## Commands

The toolchain is pinned to Rust 1.95.0 in `rust-toolchain.toml` (edition 2024, with rustfmt and clippy). CI (`.github/workflows/check.yml`) runs:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build -p omg-cli --locked && python3 -m unittest discover -s tests -p 'test_*.py' -v   # Unix PTY smoke test (Linux, macOS); needs target/debug/omg
node --test packages/oh-my-gigabytes/test/release.test.js                                     # npm launcher/signing tests (Node 22+)
```

CI runs those on nine runners (ubuntu-22.04/24.04/24.04-arm, windows-2022/latest/11-arm, macos-15/15-intel/26) in both the default and the `--no-default-features` (analysis-only) build, plus `cargo deny` and a CycloneDX schema check. Run the analysis-only build locally with `cargo test --workspace --no-default-features --locked`; it replaces `target/debug/omg`, so rebuild with `cargo build -p omg-cli --locked` before `release.test.js`. Actions are pinned by commit SHA.

Running a subset of tests:

```sh
cargo test -p omg-core --test service                      # one integration-test file (scanner, policy, planning, service)
cargo test -p omg-core --test service ipc_is_a_closed      # one test by name substring
cargo test -p omg-cli --test cli service::                 # CLI end-to-end tests (use the built binary via CARGO_BIN_EXE_omg)
cargo test -p omg-core --test migrations                   # schema 1..11 upgrade and recovery
cargo test -p omg-core --test macos                        # macOS only: hdiutil disk images (APFS, HFS+, exFAT, FAT32)
scripts/fs-sandbox.sh ext4|xfs|btrfs up|test|down          # loop-image filesystem sandbox (needs sudo and mkfs tools)
cargo run --release -p omg-core --example benchmark        # synthetic scan benchmark
python3 -m unittest discover -s spikes/phase0 -p 'test_*.py' -v   # Phase 0 Python probe (not the engine)
cargo test -p omg-core --test windows                     # Windows only: native scan and delete tests
```

Windows: CI's Windows runners build with MSVC and a static C runtime (`.cargo/config.toml`). Local builds can use `1.95.0-x86_64-pc-windows-gnu` with a MinGW linker on a path without spaces. Build from a copy on a Windows drive, not from `\\wsl.localhost`. The deletion tests skip, with a message, when `TMP` is inside a Git repository; set `TMP` to a folder outside one.

Running the app. Always use a separate private state directory for experiments. The default is `$XDG_STATE_HOME/omg` (Windows: `%LOCALAPPDATA%\omg`), or `OMG_STATE_DIR` if set.

```sh
cargo run -p omg-cli -- --state-dir /tmp/omg-state scan --root /path/to/dir
cargo run -p omg-cli -- --state-dir /tmp/omg-state status      # also: tree [PATH], findings, trends, history, doctor, service status
cargo run -p omg-cli -- --state-dir /tmp/omg-state             # TUI; needs a real terminal
```

Constraints that affect tests and running:
- **Filesystem.** Linux: scan roots and the state directory must be on ext-family, XFS, tmpfs or Btrfs; `platform::linux::root` checks the filesystem magic and requires `openat2` (Linux 5.6+). macOS: a local APFS, HFS+, exFAT or FAT volume, given by its real path (`/tmp` and `/var` are links; use `/private/tmp`); the sealed system volume is refused. Windows: a local drive-letter path on NTFS or ReFS; `platform::windows::open_root` refuses links, network drives, FAT32/exFAT and other filesystems.
- **Permissions.** On Unix the state directory must be owned by the current user with mode `0700`. Tests create it with `private_state()`. On Windows `platform::windows_state` creates it with an access list for you and SYSTEM only and refuses one open to others; Windows tests call `platform::windows_state::protect` on their temp folders. macOS tests need a canonical `TMPDIR` (CI sets it with `pwd -P`) because omg refuses links in a root or state path.

## Architecture

### Workspace

- **`crates/omg-core`** contains all logic. It has no UI.
- **`crates/omg-cli`** builds the `omg` binary:
  - `main.rs`: clap commands.
  - `tui/`: ratatui interface. `worker.rs` owns the SQLite connection and answers requests over channels; `mod.rs` holds `AppState`, menus/actions and a redraw-on-change loop; `render.rs` is pure rendering that must never query SQLite; `theme.rs` has colours, the block-letter banner and formatting.
  - `svc.rs`: service control surface, Linux only.
- **`catalog/bundled.toml`** is the known-good rule catalog. It is compiled in with `include_str!`.
- **`migrations/NNN.sql`** holds the schema migrations.
- **`packages/oh-my-gigabytes` and `scripts/`** hold the npm launcher, signed release assembly, verification and SBOM.

### `Index` is the hub

`index::Index` (`index.rs`) owns the single `rusqlite::Connection` and the state path. Other modules extend it with their own `impl Index` blocks rather than taking a connection:

| Module | Methods it adds to `Index` |
| --- | --- |
| `catalog.rs` | `catalog`, `import_catalog`, `rollback_catalog`, `findings`, ignores |
| `planning.rs` | `preview`, `record_preview`, `history`, `execute` (always denied) |
| `service/trend.rs` | `record_sample`, `prune_samples`, `trend` |
| `service/notify.rs` | Notification state persistence |

Find functionality by grepping for `impl Index`.

### Schema migrations

Migrations are applied in `Index::open` with a `PRAGMA user_version` gate per file. Adding `migrations/00N.sql` (currently up to 011) requires three edits in `index.rs`:
1. Add the `include_str!` step.
2. Bump the "schema is newer than this omg build" check (`version > N`).
3. Bump `schema_supported` in `diagnose`.

`diagnose` backs `omg doctor`. It must stay read-only: it never creates, migrates or repairs state.

### Scan and publish

(Performance notes: workers pull directories from an in-memory queue, so scanner speed comes from `scanner.rs` queue/batching, not from SQLite jobs; keep per-entry work to one `statat`. Old scans are pruned lazily via `Index::prune_stale` / `omg prune`.)

`scanner::scan` takes `scan.lock` and records a `running` row in `scans`. Bounded worker threads then traverse the tree. Entries are batched through a bounded channel into `IMMEDIATE` transactions.

The walk is platform-neutral. `platform::native` supplies the five native operations (open root, open a verified directory, read its metadata, list its children, read the volume):
- **Linux:** `openat2` (`RESOLVE_NO_SYMLINKS | BENEATH | NO_XDEV`) and one `statat` per entry.
- **macOS** (`platform::macos`): `openat` with `O_NOFOLLOW_ANY | O_RESOLVE_BENEATH` and one `statat` per entry; a different `st_dev` is a mount boundary; the sealed system volume (`MNT_ROOTFS`) and non-local mounts are refused; iCloud dataless files (`SF_DATALESS`) are recorded and never read; a Darwin kernel older than 24 (macOS 15) is refused because an older kernel could ignore the open flags. Values and behaviour: `docs/macos-evidence.md`.
- **Windows** (`platform/windows.rs`): verbatim `\\?\` paths opened with reparse points visible; children listed with `FileIdExtdDirectoryInfo`, with no per-entry open; each directory's final path and file id checked against the listing. Links, mount points and cloud placeholders are recorded as excluded and never read.

Only a `complete` scan replaces the `active` singleton, which is the published view. `partial`, `cancelled` and `failed` scans keep the previous valid index. Older scans are deleted, and a volume/trend sample is recorded after every complete or partial scan.

Entries follow these conventions:
- **Paths:** stored as **raw bytes relative to the scan root**. On Windows these are WTF-8 with `/` separators (lossless, including unpaired surrogates), displayed with `\`.
- **Identity:** `"dev:ino"`; on Windows `"volume:file-id"`. Totals deduplicate hard links with `GROUP BY identity`. Windows scans do not read link counts (`links` is 1), so hard-linked data counts once per name there.
- **Exclusions:** `excluded IS NOT NULL` marks entries skipped by mandatory or configured exclusions. Those come from `platform::excluded`.

### Rollups (`rollup.rs`, table `dir_totals`)

`rollup::Aggregator` accumulates per-directory subtree totals (hard links counted once under the first path), newest-change time and a count of `policy::hard_protection` hits while `scanner::scan` walks (workers compute the protection bit); `finish` folds and stores them before the `active` swap. `rollup::compute` is the slower reference implementation that must stay equal to it (tested). `Index::subtree` and `Index::children` read them. `""` is the scan root, and older indexes without rollups return "unavailable" rather than zero. `service/trend.rs` snapshots growth from the same table.

### Findings are derived, not stored

`Index::findings` builds findings on demand. Directory-root rules (npm, pip and uv caches) resolve their root and carry subtree totals. `large_file` comes from an indexed query and skips files already inside a directory finding. A directory containing protected descendants is itself `protected`. Findings are then sorted by `estimate` and paged. The active catalog is either `bundled.toml` or an Ed25519-signed import stored in `catalog_releases`.

`catalog::parse` admits only:
- tiers `info` or `review`
- action `info`
- a fixed enum of discovery kinds

`policy::protection` is applied independently of the catalog. Catalog tier or action never grants cleanup authority.

### Locks and coordination

All lock files live in the private state directory and use kernel advisory locks (`index::lock`):
- `database.lock` is held briefly while opening or migrating. `Index::open` waits for it.
- `scan.lock` gives a single scan owner, foreground or service.
- `service.lock` gives a single background service per profile.

Contention returns the typed error `index::Busy`. Callers use `downcast_ref::<Busy>()` to coalesce or hand off instead of failing. `index::lock_held` probes a lock without creating its file; service health is based on it, never on a PID.

### Background service (`service/`, Phase 5)

Only `config`, `notify`, `schedule` and `trend` are portable. `daemon`, `ipc` and `unit` are `#[cfg(target_os = "linux")]`.

- **`daemon::run` is scan-only.** It holds `service.lock` and runs scans on a worker thread, each with its own `Index`. It serves IPC every tick and handles backoff, the battery pause and threshold notifications.
- **IPC (`ipc.rs`) is a closed enum: `status | scan | yield | stop`.** Messages are bounded and versioned, and peers are authenticated with `SO_PEERCRED` uid. The socket is addressed through `/proc/self/fd/N/service.sock` to avoid the 108-byte socket-path limit. Never add an operation that can mutate user files.
- **Foreground handoff.** A foreground `omg scan` that hits `Busy` sends `yield` and retries; see `svc::scan_with_handoff`.
- **Registration (`unit.rs`) uses only the per-user systemd manager.** It calls fixed `systemctl --user` argv, quotes arguments systemd-style, and only overwrites or removes units carrying omg's marker line.
- **Config (`config.rs`) is strict TOML.** It uses `deny_unknown_fields`, is size-bounded and range-checked, and has notifications off by default.

## Rules that are easy to break

- **One mutation path.** User file deletion exists only in `crates/omg-core/src/cleanup.rs`: `verdict` (home-only; `policy::hard_protection` denies; `policy::repository_status`: active repo denies, build/dependency folder or stale repo warns; `Kind` decides how descendants are checked), `prepare_delete` → confirmed fingerprint → `delete_permanently` (revalidates identity *and* statx birth time, re-checks descendants, removes through directory fds). Never add another deletion path, never follow links, and keep `planning::execute` failing closed. Windows removal is the `windows` module in the same file: open without delete sharing, revalidate identity, creation time, final path and owner through that handle, then delete by handle (`FileDispositionInfoEx`). `platform/windows.rs` must stay read-only. Run `crates/omg-core/tests/cleanup.rs` (Linux) and `crates/omg-core/tests/windows.rs` (Windows) after any change there.
- **Structural guard test.** `service_logic_contains_no_deletion_or_cleanup_calls` in `crates/omg-core/tests/service.rs` greps the service sources for `remove_file`, `remove_dir`, `rename(`, `planning`, `Trash`, `unlink` and `authoriz`. **Comments count too.** Choose wording that avoids those words in `daemon.rs`, `notify.rs`, `trend.rs`, `schedule.rs` and `config.rs`.
- **Unknown is not zero.** Represent unknown sizes and identities as unknown. Keep paths as raw bytes; display strings are never path authority. Notifications and default diagnostics must not contain paths or filenames.
- **Strict, bounded parsers.** All external input — catalog, config, IPC, release manifests — is size-bounded and rejects unknown fields. Keep it that way.
- **Packaging capability coupling.** Manifest `capabilities` are produced in `scripts/assemble-release.js` and validated exactly in `packages/oh-my-gigabytes/lib/verify.js`. Change both together, and update `release.test.js`. Linux and Windows full builds declare `cleanup: development-unqualified`; Windows declares `analysis: development-windows` and `service: disabled`; macOS declares `analysis: development-macos`, `cleanup: disabled` and `service: disabled`. `assemble-release.js` defaults to the `analysis-only` variant and only runs a binary the host can execute; `scripts/sign-release.js` is the owner's multi-target signing step (the signing key never goes to CI).
- **Tests must not touch the real user session.** systemd tests use a fake `systemctl` script (`Systemd { systemctl, unit_dir, unit }`). CLI tests set `XDG_CONFIG_HOME` and `HOME` to temp directories. Never let a test write to the real `~/.config/systemd/user`.
- **Platform gating.** Linux-only code stays behind `#[cfg(target_os = "linux")]`, macOS-only code behind `#[cfg(target_os = "macos")]` and Windows-only code behind `#[cfg(windows)]`, so every CI leg compiles. There is no local Mac: macOS code is compiled and tested only by CI, so push and read the macOS legs before relying on it. Declare every `windows-sys` feature omg-core uses in its own `Cargo.toml`: `cargo test -p omg-core` builds it without the CLI's dependencies, which otherwise enable some features by accident.

## Style

- Match the existing compact Rust style: `anyhow::Result` with `bail!`, terse safety-rationale comments only where behavior is non-obvious, and `rustfmt` defaults.
- Commit subjects follow the history: one imperative line such as "Implement Phase N …".
- The source code and the independently authored catalog are licensed MIT OR Apache-2.0.
