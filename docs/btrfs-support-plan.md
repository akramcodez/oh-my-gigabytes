# Btrfs analysis support: implementation handoff

## Goal and scope

Make OMG scan and browse an explicitly selected local Btrfs directory on Fedora, with its private SQLite state directory also on Btrfs. Support metadata inventory, findings, read-only cleanup previews, and existing scan-only service behavior. Btrfs permanent deletion is not qualified and remains unsupported; OS Trash is not implemented.

This is an implementation plan, not evidence that support has been implemented or verified.

*Superseded wording (2026-10-15, WP01):* completion and handoff items below that say "cleanup stays disabled" describe the original Btrfs milestone. Current state: Btrfs scan/analysis is accepted by native evidence; Btrfs permanent deletion is unqualified and unsupported (WP02 gate); OS Trash is not implemented.

Start with one selected subvolume and preserve the scanner's existing boundary exclusions. Recursive inventory across subvolumes, snapshot management, exclusive/shared extent accounting, and enabling deletion are separate features. Do not add privileged commands, automatic mounts, filesystem formatting, or a Btrfs CLI dependency to ordinary scans.

## Context for the implementing agent

- `crates/omg-core/src/platform.rs`: `linux::root` opens a directory with `openat2`, then only accepts filesystem magic values `0xef53` (ext family) and `0x01021994` (tmpfs). Linux's `BTRFS_SUPER_MAGIC` is `0x9123683e`, verified in `/usr/include/linux/magic.h` on this machine.
- `crates/omg-core/src/index.rs`: `Index::open` calls the same root check for the state directory. This is why even `status` and the TUI fail on the current Btrfs `/home`.
- `crates/omg-core/src/scanner.rs`: entries use device/inode identity; directories with a different device are excluded, and descendant acquisition retains `NO_SYMLINKS`, `BENEATH`, and `NO_XDEV`.
- `crates/omg-core/src/rollup.rs`: totals deduplicate file identities, not shared data extents. Hard links and reflinks are different: hard links name the same file; reflinks are distinct files that can share underlying data.
- `Index::status` already exposes `scope_note`: scoped inventory is not whole-volume usage, exclusions are omitted, and shared clones/snapshots are unknown. The TUI currently does not display this field.
- Btrfs subvolumes have independent inode namespaces and can share file extents. Verify boundary behavior on the actual kernel; do not assume a subvolume is an ordinary directory or that an inode number is globally unique.

Official reference: https://btrfs.readthedocs.io/en/latest/btrfs-subvolume.html

## 1. Enable the filesystem profile

Primary file: `crates/omg-core/src/platform.rs`.

Introduce a small, testable predicate or equivalent named constants for allowed Linux filesystem magic values. Accept Btrfs alongside ext-family and tmpfs; continue rejecting every other value. Update the rejection message to include Btrfs in the supported list, ideally with the detected magic value for diagnosis.

Preserve root acquisition flags, state-directory ownership/0700 checks, exclusions, and descendant resolution restrictions. Do not replace anchored descriptor access with path-based traversal.

Completion: ordinary Btrfs scan roots and private writable Btrfs state roots are accepted; unknown filesystem types remain rejected. A read-only Btrfs scan root is valid with writable state elsewhere; read-only state must fail clearly rather than be treated as a valid writable database.

## 2. Verify boundaries and file identity

Primary files: `crates/omg-core/src/scanner.rs`, `crates/omg-core/src/platform.rs`; change these only where a real fixture exposes a gap.

Exercise a scan rooted inside a Btrfs subvolume. Verify that ordinary directories traverse, hard links deduplicate, and a reflink copy retains a distinct identity. Record actual `st_dev`/inode behavior for nested subvolumes and snapshots.

For this first version, nested subvolumes/snapshots must be excluded without scanning their contents. If current device checks do not identify a boundary, implement a narrowly scoped, metadata-only boundary check backed by fixture evidence. Do not remove `NO_XDEV` or broadly permit crossing mounts. Preserve existing handling of intentional exclusions versus unexpected acquisition errors: unexplained failures must not publish a complete scan.

Completion: the excluded boundary is recorded with an understandable reason, no child contents are indexed, and scanning that subvolume directly works. If this needs substantially broader identity or traversal changes, stop and document the unresolved case instead of claiming full Btrfs support.

## 3. Explain size limitations in the product

Primary files: `crates/omg-core/src/index.rs`, `crates/omg-cli/src/tui/render.rs`; inspect CLI tree/findings/preview output as well.

Reuse existing `scope_note` and warning mechanisms where possible. Make the relevant inventory limitation visible in the TUI and document CLI size semantics. Do not add a schema migration just to display a warning.

Distinguish apparent size (logical file length), per-file allocated size (reported metadata), and volume free space (filesystem-reported availability). Summed file allocation is not an exclusive physical usage total and is not a promise of bytes freed by deletion. Compression, snapshots and reflinks require that distinction.

Do not change hard-link deduplication into extent deduplication or invent an exclusive-space estimate. If the progress estimator gets no useful inode count on Btrfs, use the existing unknown-total presentation rather than a fabricated total.

Completion: CLI and TUI do not imply inventory totals are exact reclaimable space. Existing JSON fields stay compatible and actual cleanup stays disabled.

## 4. Add meaningful verification

Primary files: `crates/omg-core/tests/scanner.rs`, `crates/omg-core/tests/rollup.rs`, `crates/omg-cli/tests/cli.rs`; add a dedicated Btrfs integration test file if clearer.

Unit coverage must prove allowed/rejected magic classification. Real filesystem coverage must explicitly use Btrfs, not the default temp directory: `/tmp` on this Fedora machine is tmpfs. Use an opt-in environment variable such as `OMG_BTRFS_TEST_DIR` pointing to an existing writable Btrfs directory, verify its filesystem type, and create private disposable root/state fixtures inside it. Report skipped native coverage distinctly; skip is not Btrfs verification.

Required cases:

- Ordinary nested files, sparse files, hard links, symlinks, exclusions, refresh after changes, and cancellation retaining the last published scan.
- Writable Btrfs state: initialize, close, reopen, and query the SQLite index; preserve private permissions and locking behavior.
- CLI scan, status, tree, findings, history, and dry-run preview using the isolated profile. Confirm fixture files remain unchanged by previews.
- Real reflink and compressed-file fixtures, with metadata comparisons and explicit shared-allocation uncertainty. Use fixture tools only in opt-in tests; unavailable prerequisites must be reported rather than counted as passed.
- Nested subvolume and snapshot boundaries, direct scanning of a selected subvolume, and read-only scan-root handling when available. Do not create/delete a subvolume or snapshot outside the isolated fixture.

Keep fixture preparation/cleanup confined to agent-owned paths. Never scan the whole home directory for automated tests. Prefer an existing Btrfs mount; privileged disposable mounts require a separately controlled test environment.

Run `cargo fmt --all -- --check`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings`. Also run the opted-in native Btrfs cases with their documented environment. Fix introduced failures; report unrelated pre-existing failures separately.

Completion: ext/tmpfs regressions pass, native Btrfs cases actually execute, and remaining unverified cases are listed precisely. Do not describe a passing tmpfs suite as native Btrfs certification.

## 5. Document and hand off

Primary files: `README.md`, `docs/phase1-evidence.md`; check `packages/oh-my-gigabytes/README.md` for matching support claims.

Describe tested Fedora/kernel configuration, Btrfs support scope, boundary exclusions, size limitations, and disabled cleanup. Update other directly conflicting support statements only where needed. Do not claim Windows/macOS support or cross-subvolume inventory.

Report modified files, checks and native fixtures run, before/after CLI behavior, and unresolved limits. Record real results instead of pre-writing passing evidence.

Completion: an isolated Btrfs fixture supports `scan`, `status`, `tree`, and `findings` without the filesystem-profile error; the TUI opens against that index and displays its scope limitation. CLI success and TUI rendering tests alone do not substitute for a real interactive TUI check.

## Agent assignment

Implement this plan sequentially in small changes. You own the files named in each phase and any narrowly necessary test helpers. You are not alone in the codebase: preserve other people's edits and adapt to them. Read applicable repository instructions first. Do not spawn additional agents unless explicitly authorized. Inspect existing tests and warning fields before adding abstractions. Do not enable deletion, redesign the database, change traversal safety flags, or broaden filesystem acceptance beyond the named profiles. Finish with a concise implementation and verification report; if native prerequisites are missing, state exactly what remains unverified.
