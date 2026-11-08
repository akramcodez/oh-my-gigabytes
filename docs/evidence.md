# Evidence

What was verified for behaviour that is not specific to one platform. Platform pages: [Linux](linux-qualification.md), [macOS](macos-evidence.md), [Windows](windows-evidence.md). The earlier phase, planning and research pages were removed to keep the repository small; they are in Git history at `970809b`.

## Index size (2026-11-07)

The index stored every path twice and a parent copy twice, and its file never shrank: deleted scans left free pages, so after one rescan it stayed about twice the size of its data. Changes:

- Schema 12 keys `entries` by `(scan_id, parent, name)` in a `WITHOUT ROWID` table, with `path = parent||name`. The `entry_identity` and `entry_parent` indexes and the separate primary-key index are gone. Migration 012 copies only the published scan and then runs one `VACUUM`.
- New databases use `auto_vacuum=INCREMENTAL`; `Index::prune_stale_locked` hands free pages back in the same bounded slices as pruning, so it runs after every scan, at interface launch and in `omg prune`. `journal_size_limit` caps a kept write-ahead log at 32 MiB.
- The latest unpublished scan keeps its record and issues but no longer its entries and folder totals, which nothing read.
- `omg status` reports `index_bytes`. The release profile uses fat LTO, one codegen unit and strip.

Environment: Apple M5 Pro Mac, macOS 27.2, Rust 1.95.0, analysis-only release build. Old code is `e6d9d1b`, built from a clean export.

| Measurement | Before | After |
| --- | --- | --- |
| `/Applications`, 147,874 entries, after three scans | 150.0 MB | 30.4 MB |
| Synthetic tree, about 612,000 entries, first scan | 208 MB | 87 MB |
| Same tree, after three scans | 421 MB | 88 MB |
| Same tree, scan time (first, then rescans) | 2.77 s, about 3.45 s | 2.31 s, about 3.25 s |
| Existing 157.3 MB index, migrated on first open | n/a | 27.4 MB in 0.82 s |
| Release binary, macOS arm64 (gzip) | 4.85 MB (2.20 MB) | 3.76 MB (1.88 MB) |

Entry count and unique totals in `omg status` were identical between the old and new builds on the synthetic tree; `tree` output was identical between a fresh and a migrated index on `/Applications`.

Verification on that Mac: `cargo fmt --check`; clippy `-D warnings` in both builds; `cargo test --workspace --locked` and `--no-default-features` (analysis-only 77 passed, 0 failed, 1 ignored), with `OMG_REQUIRE_DISK_IMAGES=1`; PTY tests 7 run, OK, 5 skipped; `release.test.js` 18 passed, 3 skipped. New tests: exact non-UTF-8 path bytes through the migration, siblings such as `a.b` staying outside a subtree, the file shrinking after pruning, and the latest unpublished scan keeping only its record.

Not passed yet: these query changes in the delete path have run on macOS only (`crates/omg-core/tests/cleanup.rs`, see [macos-evidence.md](macos-evidence.md), "Permanent delete"), not on Linux or Windows; `tests/service.rs` and `tests/windows.rs` do not run on a Mac. A rescan still holds two generations until it publishes, so the peak during a rescan is about twice the final size (keeping the previous valid index requires this; the peak was not measured). Hard-link checks on Windows now sort instead of reading an index, and that cost has not been measured on a Windows profile. No scan above about 612,000 entries was measured, so the pruning budget at larger sizes is unverified.

## More suggestions from a scan (2026-11-07)

Requested by the owner after reviewing what took space on the development Mac. New discoveries, each a fixed kind in `catalog::DISCOVERIES`: `project_build_output`, `app_caches`, `ai_tool_old_versions`, `ai_tool_sessions`, `colima_vm`, `docker_desktop_vm`. The first four feed `Index::suggest`; listing in OMG Trash authorizes nothing, and every item still goes through a confirmed plan. Tier and action remain `review` or `info`, `action = "info"`.

Decisions:

- Build output needs proof: `target` beside `Cargo.toml`, `node_modules` beside `package.json`, a virtualenv with `pyvenv.cfg`. A folder merely called `target` is not suggested and stays an ordinary folder at deletion.
- Session transcripts cannot be recreated, so they carry a warning, need 7 days of age and 1 MB, and only `projects/<project>/*.jsonl` matches, never anything deeper such as memory.
- Container and virtual machine disks are refused by `policy::hard_protection` (docs/safety.md, "VM/container storage"). Before this change a `.colima` disk could be selected and deleted. The owner asked for these in the suggestions; they are shown as findings with the `docker system prune` route instead, because one disk holds every image, container and volume.
- `Index::auto_trash` is on by default on every platform (owner decision 2026-11-07); a user's "off" is kept. A scan fills the list, and deleting still needs a separately confirmed plan (S01, S02).
- `unused_clone` (owner request, 2026-11-07) suggests a whole repository only when `policy::pristine_clone` proves it: a remote, every branch equal to its remote branch, no loose objects (a clone keeps fetched objects in packs, so these mean a commit, a staged change or a pull), no stash, loose tag, submodule, worktree or operation in progress, no change time in the working tree after the index was written (except tool output such as `node_modules`), no unreadable folder, and 30 days untouched. This reads Git's own config and references, which is the one exception to metadata-only scanning; Git itself is never run. It can miss files that Git ignores if they were created before the index was last written, and a repository that was pulled may not qualify. No check shows that the remote still exists.
- A plan of several items refuses whole repositories (`Kind::StaleRepo`) and session transcripts with the reason "delete it on its own" (owner decision 2026-11-08), so "delete everything listed" never takes them; a plan of one item still can. Enforced in `prepare_delete`, not in the interface.
- `.dmg` files stay protected as disk images, so downloaded installers are not suggested.

Verification on the development Mac (macOS 27.2, Rust 1.95.0): `cargo test --workspace --locked` in both builds, clippy `-D warnings`, the PTY tests and `release.test.js`. New tests in `tests/cleanup.rs` cover proof and size floors, container disks, tool versions and transcripts; these run on Linux and macOS, and the application cache test on macOS only. Read-only on the real index of that Mac: `omg findings` lists the new rows.

Not passed yet: no measured use on Linux or Windows beyond the test suites; session transcripts on a real index (none on that Mac were old enough); Docker Desktop's folder, which was not present on that Mac.
