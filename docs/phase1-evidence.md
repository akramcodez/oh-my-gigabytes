# Phase 1 implementation evidence

Status: **Linux analysis development milestone implemented; full Phase 1 acceptance remains incomplete.** This is not native Windows/macOS certification or a production release. Phase 0 D14 deferred hardware tests, not mandatory scanner requirements.

## Implemented

- Rust 1.95.0 pinned workspace, `omg` executable, owner-approved MIT OR Apache-2.0 licensing and license texts.
- Linux ext-family/tmpfs profile with anchored `openat2` no-follow/no-mount descendant acquisition, raw Unix paths, device/inode identity, logical and allocated metadata.
- OMG-owned fixed worker pool (1–8), one directory stream per active worker, at most 64 queued events with 4096-byte paths; directory backlog lives in SQLite rather than an unbounded in-memory queue. Batches at most 128 events; SQLite page cache budget 4 MiB. No payload contents are opened.
- Single explicit root per profile, exclusions, last-valid/staging scan generations, atomic publication, kernel scan lock, cancellation token and CLI Ctrl-C handling, visible partial/errors.
- Bounded generation pruning, schema version checks, corruption refusal and documented preservation/rebuild guidance. Private supported local state, SQLite no-follow opening, WAL/FULL durability.
- Reconciliation detects changes/removals even when parent mtimes do not change; configured root/exclusions persisted. No unsupported mtime-based recursive cache shortcut.
- `omg scan`, `scan --full`, `status`, `doctor`, cached read-only ratatui overview, background foreground-refresh and cancel/quit controls. Service start/stop fail explicitly.

## Verification executed

On WSL2 Linux x86_64, kernel 5.15.167.4, Rust 1.95.0:

```sh
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p omg-core --example benchmark --locked
```

Six integration tests passed: hard-link/sparse/invalid-name/exclusion accounting and reconciliation; cancelled snapshot retention; redirected roots/private state denial; corrupt/future schema preservation; lock contention and interrupted-record recovery; permission-failure partial scan retains prior valid generation. Fixtures are synthetic. Interrupted-record test is not a process-kill/power-loss test.

Synthetic benchmark: 100 directories × 100 64-byte files, debug build, OS cache warm. Cold index: 10,100 entries in 1,527 ms; warm full reconciliation: 1,507 ms, zero changed entries; reopen/status/50-row query: 49 ms. Two workers, event queue bound 64, disk-backed pending-directory peak 98/99. These are one-machine observations, not representative performance targets or cold OS-cache measurements.

## Outstanding Phase 1 gates

- True incremental subtree reuse with trusted invalidation continuity and periodic/gap reconciliation. Current normal refresh deliberately re-enumerates the scope; it only identifies changed records.
- Multi-root/per-volume status and alias normalization; current profile has one root. **Directory rollups are now implemented** (schema 5, see below); they are not yet validated on representative large trees.
- Native Windows/macOS adapters and provider-specific cloud metadata testing; unsupported filesystem profiles are denied, not claimed supported.
- Adaptation to I/O pressure, actual RSS/FD/queue-byte measurements and representative cold/warm/refresh benchmarks.
- Barrier-controlled concurrent change/mount substitution tests, real process interruption, disk-full/DB fault injection, migrations from future schema revisions, complete recovery tooling.
- Cached status currently computes identity unions from SQLite; optimize persisted totals only with tested consistency. TUI rendering/PTY panic restoration needs integration qualification.
- Generic root acquisition and private-state checks reduce redirection risks but do not claim a security boundary against a hostile same-account process replacing application state.

The CI workflow compiles on all three OS families, with non-Linux acquisition disabled; it has not been run remotely here. All mandatory native and release gates remain open. Later catalog/review work may build on this development milestone, but the checklist must not call full Phase 1 complete.

## Directory rollups (added after Phase 5)

Schema 5 adds `dir_totals`: per-directory allocated/apparent bytes, file and directory counts and a count of protected/excluded entries. They are computed once per **complete** scan and published together with it (staging rows share the scan id, so a partial, cancelled or failed scan never exposes new totals and old scans' rows are cascade-pruned). A hard-linked file counts once, under its lexicographically first path; excluded entries are counted but their contents are not traversed. `omg tree [PATH]` and the TUI browse view (`b`) read them; indexes created before schema 5 report "unavailable, rescan" rather than zero.

Verification (WSL2, release build, one machine): directory totals equal an independent `std::fs` inode oracle in `crates/omg-core/tests/rollup.rs`; scanning this repository gave `target/` = 2998894592 bytes, identical to `du -s --block-size=1`; a synthetic 101 100-entry tree scanned in 10.2 s plus about 1.5 s of rollup/sample work, peak RSS 27.9 MB. Rollup memory is proportional to the number of directories, not files. A scan of an entire real home directory (431 314 entries before a deliberate 500 s timeout, about 860 entries/s) shows the scanner's own throughput on that profile is slow; that was not investigated and no performance claim is made.

## Scanner rewrite for speed (after Phase 5)

The SQLite-backed job table (two unindexed `count(*)` scans per loop and an O(done jobs) pending lookup per directory) was replaced by a shared in-memory LIFO work queue; workers send one batch per directory to a single writer that commits about 5 000 entries per transaction through cached statements, with relaxed `synchronous` only for the unpublished staging scan. Each entry now costs one no-follow `statat` (mount boundaries are detected by `st_dev` and recorded as exclusions; directories are still re-acquired with `openat2` and an identity check). Changed entries come from one join against the published scan, rollups aggregate by parent and fold upward, published totals are stored on the scan row so `status` is instant, and superseded scans are pruned in bounded chunks (`omg prune` finishes the job) instead of inside the publish transaction. `Index::open` skips the whole-file `quick_check` above 256 MiB (`omg doctor` still runs it). `--accept-gaps` publishes a scan that could not read some directories, labelled `partial`; without it a partial scan is still never published.

Measured on one WSL2 machine (release build, 16 cores): synthetic 101 100 entries 10.2 s -> 0.9 s traversal; full WSL root (about 1.56 M entries of 1.66 M used inodes) traversal 25-35 s (about 50 000 entries/s, previously about 960/s and degrading) and about 97 s end to end including rollups and publication, peak RSS about 316 MB. The earlier scan of the same filesystem was still running after 18 minutes. Post-traversal publication (join, rollup, prune) is still the larger share and is not yet optimized; these are single-machine numbers, not representative-machine benchmarks.
