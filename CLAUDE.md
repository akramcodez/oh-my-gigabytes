# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

OMG (Oh My Gigabytes) is a local-first, keyboard-first storage analyzer for developers. A Rust engine scans filesystem metadata into SQLite, a versioned catalog explains possible cleanup candidates, and an independent policy engine decides what may ever be acted on. Distribution is the npm package `oh-my-gigabytes`. The executable is always `omg`.

It is a **development preview**:
- **Cleanup is explicit and home-scoped (decision D15).** Users add paths to "OMG Trash" (a saved list; nothing moves) and can delete permanently after a confirmed plan, via `cleanup.rs`. Permanent delete runs on Linux, Windows NTFS and macOS APFS. The older `planning::execute` still always fails closed.
- **Linux, Windows and macOS.** Native scanning and state work on all three. Permanent delete exists on Linux, Windows NTFS and macOS APFS in the default build only. Windows scans NTFS and ReFS (FAT32/exFAT are refused with a message). The background service is Linux only. macOS scans APFS, HFS+, exFAT and FAT through `platform::macos` (macOS 15 or newer) and has been tested on GitHub-hosted runners and one Apple silicon Mac. Status and gaps: `docs/windows-evidence.md`, `docs/macos-evidence.md`, `docs/linux-qualification.md`.
- **First release is a development-preview beta (`0.1.0-beta.1`) and includes permanent delete on every platform (owner decisions 2026-10-24 and, for macOS, 2026-11-07).** Release builds use the default features and declare `cleanup: development-unqualified`. `--no-default-features` still builds, tests and lints the analysis-only variant. Procedure, current release state and what is left: `docs/release.md` ("Releasing the beta", "Current state", "Not passed yet").

## Source-of-truth documents

- `docs/safety.md` defines the mandatory invariants **S01–S12**. No flag, config, catalog rule or user preference may weaken them.
- `docs/evidence.md` and the platform pages (`docs/linux-qualification.md`, `docs/macos-evidence.md`, `docs/windows-evidence.md`) record what is implemented and verified, and the gates that have not passed. `docs/release.md` is the release procedure and its open items; `docs/mac-final-check.md` is the owner's hands-on Mac pass.
- Planning, phase and research documents were removed before the first release; they are in Git history.
- When documents disagree, keep the stronger safety requirement.

Documentation is evidence-based:
- Completing work means updating `docs/evidence.md` or the matching platform page. Record the environment, the exact verification commands, measured results and the gates that have not passed.
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
cargo test -p omg-core --test migrations                   # schema 1..12 upgrade and recovery
cargo test -p omg-core --test macos                        # macOS only: hdiutil disk images (APFS, HFS+, exFAT, FAT32)
scripts/fs-sandbox.sh ext4|xfs|btrfs up|test|down          # loop-image filesystem sandbox (needs sudo and mkfs tools)
cargo run --release -p omg-core --example benchmark        # synthetic scan benchmark
cargo test -p omg-core --test windows                     # Windows only: native scan and delete tests
python -m unittest discover -s tests -p 'test_terminal_windows.py' -v   # Windows only: ConPTY tests, including delete (pip install pywinpty==2.0.15; uses target/debug/omg.exe or OMG_BINARY)
```

Windows: CI's Windows runners build with MSVC and a static C runtime (`.cargo/config.toml`). Local builds can use `1.95.0-x86_64-pc-windows-gnu` with a MinGW linker on a path without spaces. Build from a copy on a Windows drive, not from `\\wsl.localhost`. The deletion tests skip, with a message, when `TMP` is inside a Git repository; set `TMP` to a folder outside one.

macOS: `scripts/mac-check.sh` runs the same steps as CI's macOS legs on a Mac. It installs Rust and Node 22 into `~/.cargo`, `~/.rustup` and `~/.local` if they are missing (no sudo). Options: `--quick` (fmt, clippy and analysis-only tests), `--smoke` (also runs the built `omg` against real paths with a private state directory), `--no-bootstrap` (fail instead of installing). It sets `TMPDIR` to its canonical path and `OMG_REQUIRE_DISK_IMAGES=1`, which a manual `cargo test` on a Mac needs too: `TMPDIR=$(cd "$TMPDIR" && pwd -P) OMG_REQUIRE_DISK_IMAGES=1 cargo test --workspace --locked`. The disk-image tests attach images with `hdiutil`, so a stuck test can leave an `omg-*` image mounted (`hdiutil info`).
The owner's hands-on pass before a release (Full Disk Access, iCloud, external drives, terminals, the CI-built package) is `docs/mac-final-check.md`.

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
  - `tui/`: ratatui interface. `worker.rs` owns the SQLite connection and answers requests over channels; `mod.rs` holds `AppState`, menus/actions and a redraw-on-change loop; `render.rs` is pure rendering that must never query SQLite (the disk gauge is refreshed every 2 s through `Request::Volume`, redrawn only when its text changes); `theme.rs` has colours, the block-letter banner and formatting.
  - `svc.rs`: service control surface, Linux only.
- **`catalog/bundled.toml`** is the known-good rule catalog. It is compiled in with `include_str!`.
- **`migrations/NNN.sql`** holds the schema migrations.
- **`packages/oh-my-gigabytes` and `scripts/`** hold the npm launcher, signed release assembly, verification and SBOM.

### `Index` is the hub

`index::Index` (`index.rs`) owns the single `rusqlite::Connection` and the state path. Other modules extend it with their own `impl Index` blocks rather than taking a connection:

| Module | Methods it adds to `Index` |
| --- | --- |
| `index.rs` | `open` (migrations), `status`, `prune_stale` (also gives free pages back), `index_bytes` |
| `catalog.rs` | `catalog`, `import_catalog`, `rollback_catalog`, `findings`, `findings_in`, ignores |
| `cleanup.rs` | `mark`, `unmark`, `marked` (OMG Trash), `suggest`, `auto_trash`, `prepare_delete`, `delete_permanently` |
| `planning.rs` | `preview`, `record_preview`, `history`, `execute` (always denied) |
| `service/trend.rs` | `record_sample`, `prune_samples`, `trend` |
| `service/notify.rs` | Notification state persistence |

Find functionality by grepping for `impl Index`.

### Schema migrations

Migrations are applied in `Index::open` with a `PRAGMA user_version` gate per file. Adding `migrations/00N.sql` (currently up to 012) requires three edits in `index.rs`:
1. Add the `include_str!` step.
2. Bump the "schema is newer than this omg build" check (`version > N`).
3. Bump `schema_supported` in `diagnose`.

`diagnose` backs `omg doctor`. It must stay read-only: it never creates, migrates or repairs state.

### Scan and publish

(Performance notes: workers pull directories from an in-memory queue, so scanner speed comes from `scanner.rs` queue/batching, not from SQLite jobs; keep per-entry work to one `statat`.)

Index size: `entries` is a `WITHOUT ROWID` table keyed by `(scan_id, parent, name)` with no secondary indexes except the partial `entry_large`; query it by key (`parent=? AND name=?`, or a `parent` range for a subtree), never by a reconstructed path. New databases use `auto_vacuum=INCREMENTAL`. `Index::prune_stale` deletes every unpublished scan's rows (the latest keeps its record and issues) and hands free pages back in bounded slices; it runs after each scan, at interface launch and in `omg prune`. `journal_size_limit` caps a kept WAL at 32 MiB.

`scanner::scan` takes `scan.lock` and records a `running` row in `scans`. Bounded worker threads then traverse the tree. Entries are batched through a bounded channel into `IMMEDIATE` transactions.

The walk is platform-neutral. `platform::native` supplies the five native operations (open root, open a verified directory, read its metadata, list its children, read the volume):
- **Linux:** `openat2` (`RESOLVE_NO_SYMLINKS | BENEATH | NO_XDEV`) and one `statat` per entry.
- **macOS** (`platform::macos`): `openat` with `O_NOFOLLOW_ANY | O_RESOLVE_BENEATH` and one `statat` per entry; a different `st_dev` is a mount boundary; the sealed system volume (`MNT_ROOTFS`) and non-local mounts are refused; iCloud dataless files (`SF_DATALESS`) are recorded and never read; a Darwin kernel older than 24 (macOS 15) is refused because an older kernel could ignore the open flags. Values and behaviour: `docs/macos-evidence.md`.
- **Windows** (`platform/windows.rs`): verbatim `\\?\` paths opened with reparse points visible; children listed with `FileIdExtdDirectoryInfo`, with no per-entry open; each directory's final path and file id checked against the listing. Links, mount points and cloud placeholders are recorded as excluded and never read.

Only a `complete` scan replaces the `active` singleton, which is the published view. `partial`, `cancelled` and `failed` scans keep the previous valid index. Older scans are deleted, and a volume/trend sample is recorded after every complete or partial scan.

Entries follow these conventions:
- **Paths:** stored as **raw bytes relative to the scan root**, split into the key `(parent, name)` with `path = parent||name` (a nested `name` keeps its leading `/`; see `index::split_path`). On Windows these are WTF-8 with `/` separators (lossless, including unpaired surrogates), displayed with `\`.
- **Identity:** `"dev:ino"`; on Windows `"volume:file-id"`. Totals deduplicate hard links with `GROUP BY identity`. Windows scans do not read link counts (`links` is 1), so hard-linked data counts once per name there.
- **Exclusions:** `excluded IS NOT NULL` marks entries skipped by mandatory or configured exclusions. Those come from `platform::excluded`.

### Rollups (`rollup.rs`, table `dir_totals`)

`rollup::Aggregator` accumulates per-directory subtree totals (hard links counted once under the first path), newest-change time and a count of `policy::hard_protection` hits while `scanner::scan` walks (workers compute the protection bit); `finish` folds and stores them before the `active` swap. `rollup::compute` is the slower reference implementation that must stay equal to it (tested). `Index::subtree` and `Index::children` read them. `""` is the scan root, and older indexes without rollups return "unavailable" rather than zero. `service/trend.rs` snapshots growth from the same table.

### Findings are derived, not stored

`Index::findings` builds findings on demand from the fixed discovery kinds in `catalog::DISCOVERIES`, in this order (an earlier finding's folder is never counted again inside a later one):
- directory roots of tool caches (npm, pip, uv, pnpm, Yarn, Cargo, Go, Gradle, Homebrew, Bun, Deno, Maven, CocoaPods, Composer, Playwright, Puppeteer, JetBrains; on macOS Xcode DerivedData, iOS DeviceSupport and Simulator caches), of downloaded ML models (Hugging Face, Ollama, LM Studio; category `ml_models`, never auto-suggested) and of container VMs (`.colima`, Docker Desktop), which `policy::hard_protection` refuses to delete;
- `unused_clone`: Git clones untouched 30 days that `policy::pristine_clone` proves have nothing local (remote, every branch on it, no loose objects, stash, local tag or later change time in the working tree);
- `stale_dependencies` (projects untouched 30 days), then `project_build_output` (50 MB+ `target` beside `Cargo.toml`, `node_modules` beside `package.json`, virtualenvs), AI coding tool old versions and session transcripts (7 days, 1 MB, `projects/<p>/*.jsonl` only), older VS Code and Cursor extension versions (`editor_old_extensions`: per id and platform suffix, all but the newest, once the newest is a day old), and on macOS app caches in `~/Library/Caches` (50 MB+, not `com.apple.*` or `GeoServices`);
- `unused_toolchain` (`catalog::toolchains`: rustup, nvm, pyenv, SDKMAN!, Android SDK, default and override locations) and `unused_app_data` (direct subfolders of `catalog::app_data_parents`, 50 MB+, nothing protected inside, not a system name in `APP_DATA_SKIP`, not overlapping a known cache or toolchain): unchanged for `catalog::UNUSED_DAYS` (150). A toolchain also needs its related folders (`.cargo`, `.gradle`) unchanged and no recent project folder of its language (`target`, `node_modules`, `__pycache__`, `build`). Toolchains are cache roots at deletion; app data stays `Kind::Plain` with a warning;
- `large_file`, which skips files already inside a directory finding.

The same `cleanup::verdict` that gates deletion decides each finding's tier, so `protected` means it really cannot be deleted. Findings are sorted by `estimate` and paged. The active catalog is either `bundled.toml` or an Ed25519-signed import stored in `catalog_releases`.

`Index::suggest` adds findings of the categories `package_caches`, `build_artifacts`, `app_caches`, `old_tool_versions`, `session_history`, `unused_clones`, `unused_apps` and `unused_toolchains` to OMG Trash after a published scan, unless the user turned `auto_trash` off (default on). `omg scan` then prints a line to stderr, so scripts that parse its JSON must not merge stderr or must pass `--no-suggest`. Listing authorizes nothing. `prepare_delete` refuses whole repositories and session transcripts in a plan of several items (`one_at_a_time`), so `D` never takes them.

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

Contention returns the typed error `index::Busy`. Callers use `downcast_ref::<Busy>()` to coalesce or hand off instead of failing. `scanner::scan` must stay strict (the foreground-to-service handoff needs an immediate `Busy`). Housekeeping (`Index::prune_stale`) holds `scan.lock` only in short slices and lets go between them; `cleanup.rs` and the interface's scan job use `index::lock_wait` (bounded by `LOCK_WAIT`) so housekeeping never makes a delete or a scan fail. `index::lock_held` probes a lock without creating its file; service health is based on it, never on a PID.

### Background service (`service/`, Phase 5)

Only `config`, `notify`, `schedule` and `trend` are portable. `daemon`, `ipc` and `unit` are `#[cfg(target_os = "linux")]`.

- **`daemon::run` is scan-only.** It holds `service.lock` and runs scans on a worker thread, each with its own `Index`. It serves IPC every tick and handles backoff, the battery pause and threshold notifications.
- **IPC (`ipc.rs`) is a closed enum: `status | scan | yield | stop`.** Messages are bounded and versioned, and peers are authenticated with `SO_PEERCRED` uid. The socket is addressed through `/proc/self/fd/N/service.sock` to avoid the 108-byte socket-path limit. Never add an operation that can mutate user files.
- **Foreground handoff.** A foreground `omg scan` that hits `Busy` sends `yield` and retries; see `svc::scan_with_handoff`.
- **Registration (`unit.rs`) uses only the per-user systemd manager.** It calls fixed `systemctl --user` argv, quotes arguments systemd-style, and only overwrites or removes units carrying omg's marker line.
- **Config (`config.rs`) is strict TOML.** It uses `deny_unknown_fields`, is size-bounded and range-checked, and has notifications off by default.

## Rules that are easy to break

- **One mutation path.** User file deletion exists only in `crates/omg-core/src/cleanup.rs`: `verdict` (home-only; `policy::hard_protection` denies; `policy::repository_status`: active repo denies, build/dependency folder or stale repo warns; `Kind` decides how descendants are checked), `prepare_delete` → confirmed fingerprint → `delete_permanently` (revalidates identity *and* statx birth time, re-checks descendants, removes through directory fds). Never add another deletion path, never follow links, and keep `planning::execute` failing closed. Linux and macOS share the `unix` module in that file; macOS also refuses cloud placeholders and locked items (`platform::macos::removal_hazard`), app bundles and standard folders, and warns inside Desktop and Documents. Windows removal is the `windows` module in the same file: open without delete sharing, revalidate identity, creation time, final path and owner through that handle, then delete by handle (`FileDispositionInfoEx`). `platform/windows.rs` must stay read-only. Run `crates/omg-core/tests/cleanup.rs` (Linux, macOS) and `crates/omg-core/tests/windows.rs` (Windows) after any change there.
- **Docker goes through Docker.** `docker.rs` talks only to a local engine (Unix socket owned by the user or root, or a Windows named pipe; remote `DOCKER_HOST` is refused) and can send only the requests `check_request` allows: ping, disk usage, remove one image or one stopped container by validated id without force, and prune build cache by id. It never removes volumes or running containers, and `Engine::remove` re-checks the whole plan and each item before removing it. Ages: build cache, tagged images and stopped containers 30 days, dangling images 7 days. Tests: `crates/omg-core/tests/docker.rs` (fake engine).
- **Git metadata is the only content read.** `policy::pristine_clone` reads `.git/config`, `HEAD` and references, bounded and without following links, to judge old clones. Never run Git, and never read any other file's contents.
- **Structural guard test.** `service_logic_contains_no_deletion_or_cleanup_calls` in `crates/omg-core/tests/service.rs` greps the service sources for `remove_file`, `remove_dir`, `rename(`, `planning`, `Trash`, `unlink` and `authoriz`. **Comments count too.** Choose wording that avoids those words in `daemon.rs`, `notify.rs`, `trend.rs`, `schedule.rs` and `config.rs`.
- **Unknown is not zero.** Represent unknown sizes and identities as unknown. Keep paths as raw bytes; display strings are never path authority. Notifications and default diagnostics must not contain paths or filenames.
- **Strict, bounded parsers.** All external input (catalog, config, IPC, release manifests) is size-bounded and rejects unknown fields. Keep it that way.
- **Packaging capability coupling.** Manifest `capabilities` are produced in `scripts/assemble-release.js` and validated exactly in `packages/oh-my-gigabytes/lib/verify.js`. Change both together, and update `release.test.js`. Full builds declare `cleanup: development-unqualified`; Windows declares `analysis: development-windows` and `service: disabled`; macOS declares `analysis: development-macos` and `service: disabled`. `assemble-release.js` defaults to the `analysis-only` variant and only runs a binary the host can execute; `scripts/sign-release.js` is the owner's multi-target signing step (the signing key never goes to CI).
- **Tests run real Git.** The clone tests in `tests/cleanup.rs` call `git` with `GIT_CONFIG_GLOBAL=/dev/null`; age files with `set_modified`, and keep Git's index newer than the change times that ageing causes.
- **Tests must not touch the real user session.** systemd tests use a fake `systemctl` script (`Systemd { systemctl, unit_dir, unit }`). CLI tests set `XDG_CONFIG_HOME` and `HOME` to temp directories. Never let a test write to the real `~/.config/systemd/user`.
- **Platform gating.** Linux-only code stays behind `#[cfg(target_os = "linux")]`, macOS-only code behind `#[cfg(target_os = "macos")]` and Windows-only code behind `#[cfg(windows)]`, so every CI leg compiles. The owner has one Apple silicon Mac; Linux, Windows and the other macOS versions are tested only by CI, so push and read those legs before relying on them. Declare every `windows-sys` feature omg-core uses in its own `Cargo.toml`: `cargo test -p omg-core` builds it without the CLI's dependencies, which otherwise enable some features by accident.

## Style

- Match the existing compact Rust style: `anyhow::Result` with `bail!`, terse safety-rationale comments only where behavior is non-obvious, and `rustfmt` defaults.
- Commit messages are one Conventional Commit line (`feat: ...`, `fix: ...`, `docs: ...`), as in the history.
- The source code and the independently authored catalog are licensed MIT OR Apache-2.0.
