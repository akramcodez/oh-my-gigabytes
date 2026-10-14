# Phase 3 implementation evidence

Status: **read-only review/dry-run milestone implemented; full Phase 3 cleanup acceptance is blocked.** No native Trash handler passed the target-race/descendant certification gate. The executor always denies cleanup, even for a forged plan. No phase-completion or “all safe to delete” claim is made.

## Implemented

- Keyboard-first ratatui findings grouped by category within pages, detail/evidence/effects/regeneration/source views, path/name search, tier/category/minimum-size filters and page navigation.
- Exact rule/path/root ignore saving, scope review/removal, local history, Linux location-open integration after protection/revalidation. The fixed `/usr/bin/xdg-open` integration is optional; it was not invoked in tests and still requires native environment qualification.
- Immutable read-only preview fingerprint binds catalog and exact item identity/path/rule/warnings. Explicit IDs required; hard-linked targets deduplicated. Info/protected findings cannot be selected for cleanup; selected actionable estimate remains zero.
- Session-local separate preview acknowledgement with cancel/back; acknowledgement records a dry-run only and cannot execute mutations. No destructive confirmation, persisted authorization token or unattended mode is implemented.
- Immediate Linux no-follow target revalidation; replaced/redirected/stale targets produce skipped decisions. Dry-run itself performs no target mutation, staging or tool cleanup.
- `omg cleanup --dry-run --candidate ID...` and `omg history`; invalid implicit/force/yes requests fail. Schema 2→3 adds bounded local preview history and truthful `evaluated-no-mutation` outcomes.
- Narrow-terminal resize guidance and persistent quit/cancel keys; cached first frame precedes refresh. Cancellation/quit joins scanner workers.

## Verification executed

On WSL2 Linux x86_64 / Rust 1.95.0:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build -p omg-cli --locked
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -p 'test_*.py' -v
```

Fifteen Rust tests passed: 6 scanner, 3 catalog, 2 policy, 1 planning, 2 CLI and 1 rendering test across 40×10, 60×20, 80×24, 120×40 terminal sizes. Planning tests prove explicit target requirement, identity deduplication, no mutation, always-denied execution, replacement revalidation and history. CLI end-to-end test exercises scan/status/findings/dry-run/history using a synthetic sparse file. Linux PTY smoke test verifies cached TUI first output, quit and restoration of terminal settings.

## Unpassed acceptance gates

- Actual Trash/Recycle Bin operation, stable native target acquisition through mutation, protected descendant checks, race/failure/cancellation/interrupt recovery and native receipts: not implemented/qualified; executor disabled.
- Real operation audit intent/outcome atomicity/fault injection and observed available-space delta: no destructive operation exists, so no actual processed/freed outcomes are claimed.
- Independent Linux/macOS user review (V01) and Windows/macOS native certification: pending.
- Full-catalog global search/category aggregation and large-detail/plan scrolling: current page-level UI is initial; exact full plan is available in CLI JSON, and execution is unavailable rather than authorizing truncated detail.
- Comprehensive event-sequence/panic restoration, hostile filename/IPC/state substitution, representative responsiveness/resource and platform file-manager tests: pending.

These blockers mean the requested complete Phases 1–3/all-good release confirmation cannot be supplied. The commits identify development milestones, not full roadmap completion. No Phase 4/service/packaging implementation has been started.

## User cleanup (decision D15, 2026-10-14)

At the owner's request, OMG now offers cleanup on Linux, scoped to the user's home folder:

- **OMG Trash** is a list of saved paths (schema 7, table `marked`). Adding an item changes nothing on disk. Items are revalidated against the live filesystem when added (identity must match the index) and record the file's birth time.
- **Delete permanently** works from OMG Trash or directly from Browse/Findings. The interface freezes an exact plan (`Index::prepare_delete`), shows every item, size, skipped item and warning, defaults to **Cancel**, and asks a second time when an item is inside a Git repository. `Index::delete_permanently` requires the plan's fingerprint, holds the cleanup and scan locks, records an intent before mutating, revalidates each target (device+inode **and** birth time, owner, type, gates), freshly re-checks every descendant of a directory for protected names and mount points, then removes contents through directory descriptors without following links. A replaced, protected or changed target is skipped, never retried. The published index is adjusted for removed subtrees without a rescan.
- **Always denied:** anything outside home, the home folder itself, links/special files as targets, mandatory exclusions (cloud, credentials, `.ssh`, `.git`...), databases, disk images, keys, active downloads. Being inside a Git repository is a warning, not a block; `.git` itself is always protected.
- Found by testing: ext4 reuses a freed inode number immediately, so device+inode alone deleted a recreated file in a test. Birth time (statx) is now compared too; filesystems without birth time fall back to identity only, which is a documented residual risk. Removing a single file is still path-based after the checks (Linux has no unlink-by-descriptor); directory contents are descriptor-anchored and the final `rmdir` only removes an empty directory.
- `planning::execute` (the older Trash-plan path) still always fails closed. Native OS Trash integration was not implemented; permanent deletion frees space immediately. On WSL the Windows drive regains space only after the virtual disk is compacted, which the TUI explains.

Verification: `crates/omg-core/tests/cleanup.rs` (saved-paths-only marking, fingerprint requirement, link not followed, replaced target skipped, protected/outside-home denied, Git warning, `.git` never deleted, index adjusted) and an end-to-end PTY run of the TUI deleting a folder in a temporary home.
