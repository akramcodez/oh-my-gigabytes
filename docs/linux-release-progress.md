# Linux release progress

Tracks work packages in `docs/linux-release-implementation-plan.md`. A package is marked complete only after its acceptance checks pass.

Last updated: 2026-10-15.

## Current position

- **WP01:** docs reconciled. Some historical items are deferred (see WP01 section).
- **WP02:** implemented and committed on `main`. Partial: no TUI capability display; the non-Linux build was not checked locally (CI only).
- **WP03 baseline:** recorded before any code change (see WP03 section).
- **Plan:** `docs/linux-release-implementation-plan.md` updated 2026-10-15 with every remaining Linux item (status table, owner decisions, "Remaining"/"Added" lists per WP, prerequisites).
- **WP04:** core done (session-bound, one-use plans; see WP04 section). Open: owner decision on auto-fill.
- **Next:** WP06 (durable outcomes), the next package that does not need an owner decision or native host. WP05 waits for an ext4 host.

## Owner decisions

- **2026-10-15, branches:** all work is committed directly on `main`; no feature branches.

- **2026-10-15, filesystem gate (D16 in `plans.md`):** option 2. Permanent deletion is allowed on ext-family, tmpfs and Btrfs, labelled `development-unqualified`. Unknown filesystems are refused. No bypass flag. This departs from the plan's strict reading ("unqualified denies") because the owner's home is Btrfs and the owner accepts the shared-extent size uncertainty.

## Capability truth (traced to code, 2026-10-15)

| Capability | Code path | Scope | Status |
| --- | --- | --- | --- |
| Scan and analysis | `scanner.rs`, `index.rs`, `catalog.rs`, `platform::linux::root` | Linux; ext-family, tmpfs, Btrfs roots | Development implementation. Btrfs accepted by native scan evidence only (prior session) |
| Saved-path list ("OMG Trash") | `cleanup::Index::mark`, `marked`, `unmark`; TUI Space; `omg trash` | Saves paths only; nothing on disk changes | Implemented. **Also auto-filled by scans** (Issue 1) |
| Auto-suggest into OMG Trash | `cleanup::Index::suggest`, `auto_trash` (**on by default**), `set_auto_trash`; `omg trash --refresh`; scan step; TUI menu "Auto-add caches to OMG Trash on scan" | Writes `source='auto'` rows into the saved list | Implemented. Not user-initiated |
| Permanent deletion | `cleanup::Index::prepare_delete` → `delete_permanently`; TUI `d` (one item, adds it to the list first), `D` (everything in the list) | Linux; inside user home; confirmed plan; identity + birth-time revalidation; directory-descriptor removal; gated by `capability` | **`development-unqualified`.** Not recoverable |
| Older Trash-plan executor | `planning::execute` | Always bails: "OS Trash is not implemented…" | Denies. Does not disable permanent deletion |
| OS Trash and restore | None | — | **Not implemented** (WP09). `capability::os_trash()` = `unsupported` |
| Dry-run preview and history | `omg cleanup --dry-run --candidate`, `omg history`, `Index::preview`, `record_preview` | Explicit IDs | Implemented; read-only |
| Location open (`xdg-open`) | `planning.rs` (`/usr/bin/xdg-open`), TUI open-location action | Opens the file manager after protection and revalidation | Implemented; not exercised in tests; needs native check (WP10) |
| Catalog import / rollback | `omg catalog import PAYLOAD SIGNATURE`, `omg catalog rollback`; `catalog.rs` | Ed25519-signed catalog | Implemented. Import disabled in normal installs until an owner-held release key exists (phase2-evidence) |
| Scan-only service | `omg start`/`stop`/`service install`/`uninstall`; `service/*`; `svc.rs` | systemd user manager, Linux only; registration needs explicit approval | Development implementation. IPC is scan-only (`status`/`scan`/`yield`/`stop`) |
| Public release and platforms | `docs/release.md`, `packages/`, `scripts/` | — | **Not published.** No production signing key. Windows/macOS not certified (stub build only) |

## Issues (open)

1. **Saved list is not only user-populated (WP04, WP10).** Auto-fill is on by default. `D` deletes everything in the list after one confirmation, so it can delete items the user never picked. Owner question pending: default off, or separate confirmation for auto-filled items.
2. **`DeleteNow` stages before preparing (WP10).** It adds the item to OMG Trash, then calls `prepare_delete`. If preparation fails (for example, unsupported root or moved scan root), the item stays in OMG Trash and the list does not refresh, because `refresh` is set only on success.
3. **Birth-time fallback (WP05).** `cleanup.rs` compares `statx` birth time only when the filesystem reports it. Otherwise revalidation is identity-only (Phase 3 residual risk). Must not qualify a profile. Today no profile is qualified.
4. **Wording (WP10).** Some help text still describes cleanup in older terms. The OS Trash denial text is now accurate.
5. **TUI capability display (WP10).** The TUI shows the backend error text when deletion is refused. It does not show capability status.

## Resolved in WP02 (no longer open)

- Manifest/runtime mismatch: manifest now declares `cleanup: development-unqualified` on Linux, `disabled` elsewhere. `assemble-release.js`, `verify.js`, `release.test.js` and `docs/release.md` changed together.
- Btrfs gate: owner chose option 2 (see Owner decisions).
- Status JSON still said "cleanup: disabled": now `cleanup` and `os_trash` come from `capability`.

## Work package status

| WP | Status | Notes |
| --- | --- | --- |
| WP01 | Closed for doc edits | Deferred items below |
| WP02 | Implemented, partial | On `main`. Missing: TUI capability display; non-Linux build check (CI only) |
| WP03 | Baseline recorded | Before code changes. Full gate still needs ext4 |
| WP04–WP15 | Not started | Some need owner or native prerequisites (Blockers) |

## WP01 — changes made (docs only)

- `README.md`: "Cleanup is disabled" replaced with the capability truth.
- `docs/safety.md`: current-implementation note. D15 adds a Linux, home-only, permanent deletion flow beside S07 (Trash-only for ordinary cleanup). S02, S03, S06, S10, S11 still apply.
- `docs/phase3-evidence.md`: status clarified; WP01 and WP02 notes added.
- `docs/checklist.md` (Phase 3 line): "cleanup is disabled" replaced.
- `docs/btrfs-support-plan.md`: goal section corrected; superseded-wording note.
- `docs/phase1-evidence.md`, `docs/phase2-evidence.md`, `docs/phase5-evidence.md`: milestone-time "disabled" labelled.
- `docs/phase4-evidence.md`: superseded note added (WP02).
- `docs/release.md`: status line and manifest section updated (WP02).

### Deferred WP01 items

- `docs/btrfs-support-plan.md` completion items (~lines 52, 78): "cleanup stays disabled". The top note covers them.
- `docs/README.md`: baseline date still 2026-10-13. Add a capability line.
- `docs/plans.md`: D15 (line ~76) is correct. Lines ~50 and ~109 are design-time text.

## WP02 — capability contract (2026-10-15)

Changes:
- `crates/omg-core/src/capability.rs` (new): `Support` (`unsupported` / `development-unqualified` / `qualified`), `Filesystem` (from statfs magic; `Ext` = ext2/3/4), `permanent_delete(fs)`, `os_trash()` (always `unsupported`), `platform_permanent_delete()` (cfg-aware, used by doctor, status JSON and manifest). Unit tests: 3.
- `crates/omg-core/src/platform.rs`: root acquisition uses `capability::Filesystem`; new `linux::filesystem(fd)`. Same accepted set as before.
- `crates/omg-core/src/cleanup.rs`: `delete_support(root)` checked in `prepare_delete` (before a plan is built) and in `delete_permanently` (before the intent record). A forged plan cannot bypass it.
- `crates/omg-core/tests/cleanup.rs`: new test `unsupported_root_refuses_plan_and_delete_with_zero_mutations`. It scans a tmpfs fixture, builds a valid plan, rewrites the active scan root to `/proc`, then checks that both calls error, that `operations` has 0 rows, and that the file survives.
- `crates/omg-cli/src/main.rs`: `omg doctor` (the real path, before the early return) prints `permanent delete: development-unqualified (Linux: ext-family, tmpfs, Btrfs; unknown filesystems refused); OS Trash: unsupported`. Removed a dead duplicate doctor arm.
- `crates/omg-core/src/index.rs`: status JSON `cleanup` and `os_trash` from `capability`.
- `crates/omg-core/src/planning.rs`, `policy.rs`: denial text says "OS Trash is not implemented" (accurate); tests updated.
- Manifest: `scripts/assemble-release.js` and `packages/oh-my-gigabytes/lib/verify.js` declare `cleanup: development-unqualified` on Linux targets, `disabled` elsewhere. `release.test.js` updated.

Checks (2026-10-15, final run after all WP02 edits):
- `cargo fmt --all -- --check`: exit 0
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0
- `cargo test --workspace --locked`: exit 0, 0 failures. Passed per binary: 7, 9, 7, 7, 1, 3, 5, 9, 20. Baseline had 7, 9, 4, 6, 1, 3, 5, 9, 20. The changes are +3 unit tests (`capability.rs`) and +1 cleanup test (`unsupported_root_refuses_plan_and_delete_with_zero_mutations`).
- `cargo build -p omg-cli --locked`: exit 0
- PTY `tests/test_terminal.py`: OK
- `node --test packages/oh-my-gigabytes/test/release.test.js`: 10 pass, 0 fail, 0 skipped. (First run failed on a dead-arm doctor string; fixed.)
- Runtime: `omg doctor` on a throwaway state dir prints the new lines.

Acceptance status:
- Backend refuses unsupported filesystems: **done.** Tested through the `/proc` root test. Ordering verified: the refusal happens before any intent row.
- Analysis still works on enabled profiles: yes (scan/cli tests pass).
- Btrfs analysis vs deletion distinguished: `capability` unit test asserts Btrfs is `development-unqualified`, never `qualified`. No profile is `qualified`.
- Manifest/runtime agreement: `release.test.js` checks the manifest value and the doctor string; `verify.js` rejects a mismatch.
- UI refusal: **partial.** The worker turns backend errors into `Response::Error` and keeps running (verified in `worker.rs`). No TUI test covers it.

## WP04 — exact, one-use confirmation (2026-10-15)

Changes (`crates/omg-core/src/cleanup.rs`, `index.rs`):
- `prepare_delete` now takes `&mut self` and stores the plan it issues in `Index.issued` (memory only), keyed by fingerprint, with the scan id.
- The fingerprint now covers scan id, items (including warnings), denials and creation time.
- `delete_permanently` calls `take_issued`, which removes the stored plan before any check. It refuses: an unknown or already-used plan, a future-dated plan, an expired plan (900 s), and any difference between the caller's plan and the issued one. It also refuses if the active scan changed since review.
- Stored plans older than the TTL are pruned on each new `prepare_delete`.

Checks: fmt, clippy (`-D warnings`) and `cargo test --workspace --locked` all pass. New tests: four unit tests in `cleanup.rs` (replay, modified warning, expired and future-dated, unknown fingerprint) and one integration test, `reviewed_plan_is_one_use_and_does_not_survive_a_new_session`.

Acceptance:
- Altered, expired, future-dated, replayed and warning-stripped plans are refused before any mutation (unit and integration tests).
- Cancel stays the default in the TUI (existing test `delete_confirmation_defaults_to_cancel_and_shows_warnings`).
- A plan from another session is refused (integration test).

Not done:
- **Auto-fill default** (owner decision). `D` still deletes everything in the list after one confirmation, and auto-filled items are not marked separately (WP10).
- Cancel in the TUI does not tell the backend to discard the plan. The plan stays in memory until it expires or the session ends. Harmless: the TUI no longer holds the fingerprint, but a discard call would be cleaner.
- Concurrent reuse across two TUI processes cannot happen, because each process has its own in-memory store. Within one process the `&mut self` borrow serialises calls.

## Session 2026-10-15 (second pass): remaining work packages

Commits on `main`: `05e624a` (WP04), `cc54459` (WP06/WP07/WP10 parts). Further changes are uncommitted at the time of writing; see git log.

| WP | Result | Evidence |
| --- | --- | --- |
| WP04 | Done. Auto-fill off by default; Cancel discards the issued plan; a failed or empty review undoes staging | Unit and integration tests; TUI test with real Cancel action |
| WP05 | Partial. Barrier tests via `delete_with_control`: replaced directory, symlink swap, protected child inserted | `tests/cleanup.rs` (deterministic hooks; tmpfs) |
| WP06 | Core done. Migration 011 (`delete_targets`); intent and targets in one transaction; per-target outcomes; interrupted labelling; cancel flag; partial labelled `partial` | Tests: failed intent write, cancellation, interrupted attempt, partial removal |
| WP07 | Partial. Hard-link deletion totals equal an independent fresh scan; partial removal flags `reconcile_needed`, cleared by the next complete scan | Test `deleting_one_hard_link_name_keeps_data_and_totals_match_a_fresh_scan` |
| WP08 | Synthetic benchmark only: 10,100 entries, cold 27 ms, warm 27 ms, startup query 3 ms, rollup read 0 ms (`cargo run --release -p omg-core --example benchmark`, this host) | Not representative; no RSS/FD measurement; tool-root overrides not implemented |
| WP09 | Investigation delivered. Source: freedesktop Trash spec, fetched via agent-reach (Jina Reader) 2026-10-15. Race not resolved; OS Trash kept disabled | `docs/linux-os-trash-investigation.md` |
| WP10 | Done: auto-filled items labelled in Trash tab; failed `DeleteNow` review undoes staging; capability on help screen (from capability module); cancel reported; stale totals reported | Render and worker changes; CLI tests |
| WP11 | Findings, not qualification: `notify-send` fails here (`undefined symbol: notify_notification_get_activation_app_launch_context`); D-Bus notification server accepted a test message via `busctl`. Second user, WSL, journald, idle budget missing | Command output in this session |
| WP12 | Matrix written with observed evidence: birth time reported on tmpfs and Btrfs; ext4, WSL2, ARM64, oldest kernel not available | `docs/linux-qualification.md` |
| WP13 | Partial: `artifacts.yml` restricted to Linux x86_64 GNU (macOS/Windows stay compile checks). Not run remotely. Actions not SHA-pinned | Workflow edit |
| WP14 | Partial: governance doc and `SECURITY.md` (no maintainer contact invented). Owner items open | `docs/linux-release-governance.md`, `SECURITY.md` |
| WP15 | Decision recorded: not a release candidate; not published | `docs/linux-release-checklist.md` |

Also in this session:
- CLI: a bare `omg` without a terminal now refuses before creating state. A test found that it previously created the state directory first.
- Checks: fmt, clippy (`-D warnings`), `cargo test --workspace --locked` (all 10 suites, 0 failures, 16 cleanup tests), build, PTY (1 OK), Node release tests (10/10).

Owner items still open: auto-fill default (decided off here, reversible), security contact and private reporting, signing key and publisher identity, native ext4 and WSL2 hosts, second user account, `notify-send` repair or D-Bus change, sudo for the leftover Btrfs subvolume, authorization to publish.

## WP03 — baseline (2026-10-15, before any code change)

Environment:
- Kernel: `7.2.9-201.fc44.x86_64`; Fedora 44; CPU x86_64 (11th Gen Intel i5-11400H)
- Repo filesystem: `btrfs` (`/dev/nvme0n1p3`, root subvol, `compress=zstd:1`)
- `/tmp`: `tmpfs`; `$HOME`: `btrfs`
- Rust `1.95.0` (`rust-toolchain.toml`); Cargo 1.95.0
- Node `v22.23.2`; Python `3.14.8`
- No ext4 filesystem on this host.

Commands and results (log: `/tmp/omg-baseline/baseline.log`, not in repo):

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| `cargo test --workspace --locked` | exit 0 |
| `cargo build -p omg-cli --locked` | exit 0 |
| `python3 -m unittest discover -s tests -p 'test_*.py' -v` | OK, 1 test |
| `node --test packages/oh-my-gigabytes/test/release.test.js` | 10 pass, 0 fail, 0 skipped |

Ignored tests (not run; each needs `OMG_BTRFS_TEST_DIR`):
- `crates/omg-core/tests/scanner.rs:155` `btrfs_root_and_state_are_scannable`
- `crates/omg-cli/tests/cli.rs:86` `btrfs_cli_profile_scans_and_previews_without_mutation`
- `crates/omg-cli/src/tui/tests.rs:162` `preview_screens`

The Btrfs tests were not re-run. They create a nested subvolume, and that cleanup is still pending from the handoff.

Gaps: cleanup tests run on tmpfs, not ext4 or Btrfs. Those are native gates still open.

## Blockers and owner inputs

| Item | Needed from | Blocks |
| --- | --- | --- |
| Auto-fill default: off, or separate confirmation for auto-filled items? | Owner | WP04 design |
| Remove leftover Btrfs test subvolume `.btrfs-probe.B3NNXh/nested` (`sudo btrfs subvolume delete`, then `rmdir`) | Owner | Clean fixture state; further Btrfs tests |
| ext4 native test environment | Owner | WP03 full gate; WP05 native cases |
| Non-Linux build check (macOS/Windows stub) | CI | WP02 acceptance for other platforms |
| WSL2 host | Owner | WP12 WSL profile |
| Production release signing key and publisher identity | Owner | WP13–WP15 |
| Authority to publish | Owner | WP15; never without explicit approval |
| OS Trash research (freedesktop Trash spec, chosen crate) | Agent, via `agent-reach` skill (CLAUDE.md) | WP09 |

## Next package

WP04: make confirmation exact and one-use (`cleanup.rs`, `tui/mod.rs`, `tui/worker.rs`), including the "Added" items in the plan. Start after the owner decides the auto-fill default.


## Signed local Linux artifact (2026-10-15)

- Analysis-only build signed with the dry-run key: `dist/oh-my-gigabytes-0.1.0-linux-x64-gnu/artifact/omg`, SHA-256 `5fdc6e4c3c4f64b5b0df0fff19f68fb0799d03d0866065add07ed4859462cd4c`.
- Manifest claims `cleanup: disabled`. The release script now checks this against the binary's own `doctor` output before signing (commit `5609f81`).
- Verified with `scripts/verify-release.js`.
- Remaining before upload: older-glibc build (WP13), owner-held signing key (WP14), explicit publishing authorization (WP15).
