# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

OMG (Oh My Gigabytes) is a local-first, keyboard-first storage analyzer for developers. A Rust engine scans filesystem metadata into SQLite, a versioned catalog explains possible cleanup candidates, and an independent policy engine decides what may ever be acted on. Distribution is the npm package `oh-my-gigabytes`. The executable is always `omg`.

It is a **development preview**:
- **Cleanup is disabled.** `Index::execute` always fails closed.
- **Linux only.** Native scanning, state and the background service work only on Linux. Other targets compile with those adapters disabled.
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
cargo build -p omg-cli --locked && python3 -m unittest discover -s tests -p 'test_*.py' -v   # Linux PTY smoke test; needs target/debug/omg
node --test packages/oh-my-gigabytes/test/release.test.js                                     # npm launcher/signing tests (Node 22+)
```

Running a subset of tests:

```sh
cargo test -p omg-core --test service                      # one integration-test file (scanner, policy, planning, service)
cargo test -p omg-core --test service ipc_is_a_closed      # one test by name substring
cargo test -p omg-cli --test cli service::                 # CLI end-to-end tests (use the built binary via CARGO_BIN_EXE_omg)
cargo run --release -p omg-core --example benchmark        # synthetic scan benchmark
python3 -m unittest discover -s spikes/phase0 -p 'test_*.py' -v   # Phase 0 Python probe (not the engine)
```

Running the app. Always use a separate private state directory for experiments. The default is `$XDG_STATE_HOME/omg`, or `OMG_STATE_DIR` if set.

```sh
cargo run -p omg-cli -- --state-dir /tmp/omg-state scan --root /path/to/dir
cargo run -p omg-cli -- --state-dir /tmp/omg-state status      # also: findings, trends, history, doctor, service status
cargo run -p omg-cli -- --state-dir /tmp/omg-state             # TUI; needs a real terminal
```

Constraints that affect tests and running:
- **Filesystem.** Scan roots and the state directory must be on ext-family or tmpfs filesystems. `platform::linux::root` checks the filesystem magic and requires `openat2`.
- **Permissions.** The state directory must be owned by the current user with mode `0700`. Tests create it with `private_state()`.

## Architecture

### Workspace

- **`crates/omg-core`** contains all logic. It has no UI.
- **`crates/omg-cli`** builds the `omg` binary:
  - `main.rs`: clap commands.
  - `tui.rs`: ratatui interface.
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

Migrations are applied in `Index::open` with a `PRAGMA user_version` gate per file. Adding `migrations/00N.sql` requires three edits in `index.rs`:
1. Add the `include_str!` step.
2. Bump the "schema is newer than this omg build" check (`version > N`).
3. Bump `schema_supported` in `diagnose`.

`diagnose` backs `omg doctor`. It must stay read-only: it never creates, migrates or repairs state.

### Scan and publish

`scanner::scan` takes `scan.lock` and records a `running` row in `scans`. Bounded worker threads then traverse with `openat2` (`RESOLVE_NO_SYMLINKS | BENEATH | NO_XDEV`). Entries are batched through a bounded channel into `IMMEDIATE` transactions.

Only a `complete` scan replaces the `active` singleton, which is the published view. `partial`, `cancelled` and `failed` scans keep the previous valid index. Older scans are deleted, and a volume/trend sample is recorded after every complete or partial scan.

Entries follow these conventions:
- **Paths:** stored as **raw bytes relative to the scan root**.
- **Identity:** `"dev:ino"`. Totals deduplicate hard links with `GROUP BY identity`.
- **Exclusions:** `excluded IS NOT NULL` marks entries skipped by mandatory or configured exclusions. Those come from `platform::excluded`.

### Findings are derived, not stored

`Index::findings` pages through active entries and evaluates them against the active catalog. The active catalog is either `bundled.toml` or an Ed25519-signed import stored in `catalog_releases`.

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

- **No mutation paths.** Do not add code that deletes, renames or trashes user files, or that bypasses `policy` or `planning::execute`. Phase 3 Trash certification is a blocking gate.
- **Structural guard test.** `service_logic_contains_no_deletion_or_cleanup_calls` in `crates/omg-core/tests/service.rs` greps the service sources for `remove_file`, `remove_dir`, `rename(`, `planning`, `Trash`, `unlink` and `authoriz`. **Comments count too.** Choose wording that avoids those words in `daemon.rs`, `notify.rs`, `trend.rs`, `schedule.rs` and `config.rs`.
- **Unknown is not zero.** Represent unknown sizes and identities as unknown. Keep paths as raw bytes; display strings are never path authority. Notifications and default diagnostics must not contain paths or filenames.
- **Strict, bounded parsers.** All external input — catalog, config, IPC, release manifests — is size-bounded and rejects unknown fields. Keep it that way.
- **Packaging capability coupling.** Manifest `capabilities` are produced in `scripts/assemble-release.js` and validated exactly in `packages/oh-my-gigabytes/lib/verify.js`. Change both together, and update `release.test.js`.
- **Tests must not touch the real user session.** systemd tests use a fake `systemctl` script (`Systemd { systemctl, unit_dir, unit }`). CLI tests set `XDG_CONFIG_HOME` and `HOME` to temp directories. Never let a test write to the real `~/.config/systemd/user`.
- **Linux gating.** Linux-only code must stay behind `#[cfg(target_os = "linux")]`, so the macOS and Windows CI legs still compile with those features disabled.

## Style

- Match the existing compact Rust style: `anyhow::Result` with `bail!`, terse safety-rationale comments only where behavior is non-obvious, and `rustfmt` defaults.
- Commit subjects follow the history: one imperative line such as "Implement Phase N …".
- The source code and the independently authored catalog are licensed MIT OR Apache-2.0.
