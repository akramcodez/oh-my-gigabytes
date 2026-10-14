# Linux release implementation plan

Status: implementation in progress, updated 2026-10-15. WP01 is closed for docs, WP02 is implemented but partial, and the WP03 baseline is recorded. Nothing here is certified or qualified. A plan reduces risk; it cannot guarantee absence of defects. Day-to-day results live in [linux-release-progress.md](linux-release-progress.md).

## Scope and current truth

Deliver a supported Linux release with accurate inventory, useful findings, truthful operation outcomes, qualified cleanup capabilities, and verified installation/upgrade/uninstall. WSL2 is a separately tested Linux environment, not proof of native Windows support. Native Windows/macOS adapters remain subsequent workstreams.

Current code provides metadata scans, SQLite indexing, TUI browsing/findings, cache suggestions, a saved-paths list called OMG Trash (auto-filled by scans by default), confirmed Linux permanent deletion inside the user's home, and an optional scan-only systemd user service. `planning::execute` still denies the older native-Trash execution path; it does not disable `cleanup::Index::delete_permanently`. Since WP02, permanent deletion is gated by `capability::permanent_delete` and reported as `development-unqualified`.

The recent Btrfs work accepts Btrfs scan/state directories and has native scan/CLI evidence. It did not qualify Btrfs permanent deletion. Preserve decision D15 in `docs/plans.md`; do not quietly replace permanent deletion with OS Trash or promise recoverability for permanent deletion.

Intended first qualification profiles: Linux x86_64 GNU/glibc on real ext4 and Fedora Btrfs; tmpfs for analysis/testing with cleanup qualification separate; WSL2 ext4 for analysis and separately qualified cleanup. Windows-mounted WSL paths such as `/mnt/c`, remote/cloud storage, unsupported filesystems, musl, and untested architectures are not enabled merely because a binary builds. ARM64 GNU is an additional release target only after native build/run evidence.

## Owner decisions

| Date | Decision | Effect on this plan |
| --- | --- | --- |
| 2026-10-14 | D15: "OMG Trash" saved list plus explicit permanent deletion (Linux, home only) | Permanent deletion stays. OS Trash is a separate, additional capability (WP09) |
| 2026-10-15 | D16: permanent deletion allowed on ext-family, tmpfs **and Btrfs**, labelled `development-unqualified`; unknown filesystems refused; no bypass flag | Overrides the stricter "unqualified denies" reading in WP02 for these three families. A profile still cannot be called *qualified* until WP05/WP12 evidence exists |
| 2026-10-15 | All work is committed directly on `main`; no feature branches | Applies to every work package |

Pending owner decisions are listed under [Prerequisites](#owner-and-environment-prerequisites).

## Status at a glance

| WP | Title | Status |
| --- | --- | --- |
| WP01 | Reconcile implementation status | Closed for docs; small deferred items listed |
| WP02 | Separate support from cleanup qualification | Implemented, partial (TUI display, non-Linux CI check) |
| WP03 | Record reproducible baseline | Baseline commands recorded; remaining checks listed |
| WP04 | Make confirmation exact and one-use | Core implemented 2026-10-15; auto-fill default is an owner decision |
| WP05 | Prove target and descendant safety | Not started; needs ext4 host |
| WP06 | Durable per-target outcomes and cancellation | Not started |
| WP07 | Correct inventory after mutation and failures | Not started |
| WP08 | Qualify findings and scanner scaling | Not started |
| WP09 | Investigate and qualify Linux OS Trash | Not started |
| WP10 | Clear action and outcome UI | Not started |
| WP11 | Qualify optional scan-only service | Not started |
| WP12 | Publish a tested compatibility matrix | Not started; needs ext4 and WSL2 hosts |
| WP13 | Build and install reproducible artifacts | Not started |
| WP14 | Upgrade, trust, and release governance | Not started; needs production key owner |
| WP15 | Assemble the release decision | Not started; needs publish authority |

## Execution rules for a small agent

Work through the numbered work packages sequentially. Each package is one bounded implementation session; start a new session when context becomes crowded. Read relevant code, existing tests and applicable instructions first. You are not alone in the codebase: preserve existing edits and adapt to them. Do not spawn agents unless authorized. **Commit on `main`; do not create feature branches** (owner decision). Do not push unless asked.

Maintain `docs/linux-release-progress.md` with each work-package status, changed files, exact checks/results, unresolved limitations, and the next package. Mark completed work only after its acceptance checks pass. Record missing native prerequisites as unverified, never passed. Investigating a package may show an existing implementation already satisfies it; retain that implementation and add meaningful evidence rather than rewriting it. When a package changes behavior, also update the matching `docs/phaseN-evidence.md` and, when a deliverable truly passes, `docs/checklist.md`.

Use private disposable fixtures and explicit profiles. Do not run destructive tests on a real home, project, OS directory, or user's Trash. Before creating a Btrfs subvolume/snapshot, establish that the controlled fixture can also be removed; creation permission alone does not imply deletion permission. Do not format devices, remount the user's filesystems, compact WSL disks, or install a service implicitly. Privileged integration environments must be explicitly controlled by their owner.

Resolve routine design choices within the stated scope. Continue independent work when an external prerequisite is missing. Stop the affected capability if its safety conditions cannot be established; do not invent bypass flags. Public publishing, production key provisioning, and broader policy changes need the corresponding owner decisions, not guessed values. Prepare concrete reviewable artifacts first.

Repository rules that apply to every package (see `CLAUDE.md`): one mutation path (`cleanup.rs`); the service guard test greps service sources, comments included; unknown is not zero; strict bounded parsers; change manifest capabilities in `assemble-release.js`, `verify.js` and `release.test.js` together; tests never touch the real user session; Linux-only code stays behind `#[cfg(target_os = "linux")]`.

## Stage 1 — Truthful baseline and capabilities

### WP01 — Reconcile implementation status

**Status: closed for docs (2026-10-15).**

Files: `README.md`, `docs/README.md`, `docs/checklist.md`, `docs/plans.md`, `docs/phase1-evidence.md` through `docs/phase5-evidence.md`, `docs/release.md`, `docs/safety.md`, `docs/btrfs-support-plan.md`.

Trace existing commands and TUI actions to code. Separate analysis, saved-path marking, permanent deletion, OS Trash, service, and public distribution. Correct contradictory statements without erasing historical evidence: label superseded milestone sections and state the latest behavior. In particular, remove the false blanket claim that all deletion is disabled, while keeping the older Trash executor's actual denial documented. Reconcile protection/repository rules with D15 and current code; unexplained differences remain explicit issues, not inferred safety.

Acceptance: every advertised action has a matching code path and scope; current capability statements agree across the docs. The progress document identifies implemented, qualified, unverified and deferred items separately.

Done later on 2026-10-15: `docs/README.md` status line updated; `docs/plans.md` decision D16 records the Btrfs decision.

Remaining (docs only, optional):
- `docs/btrfs-support-plan.md` completion items (about lines 52 and 78) still say "cleanup stays disabled"; the note at the top labels them, but reword if the file is edited again.

### WP02 — Separate support from cleanup qualification

**Status: implemented, partial (2026-10-15).** Done: `crates/omg-core/src/capability.rs`; backend gate in `prepare_delete` and before the intent record in `delete_permanently`; `omg doctor`, status JSON and the signed manifest all report `development-unqualified`; test `unsupported_root_refuses_plan_and_delete_with_zero_mutations`.

Files: `platform.rs`, `cleanup.rs`, `policy.rs`, `model.rs`, `index.rs`, CLI/TUI capability displays, package manifest validation/assembly and matching tests.

Introduce a small centralized capability contract for analysis, permanent deletion, OS Trash and service support. Qualify cleanup by actual environment/filesystem and target constraints, not merely the scanner allowlist. Enforce decisions in the backend even if UI or cached data claims otherwise. Unknown/missing qualification denies that action while permitting supported analysis (owner decision 2026-10-15: ext-family, tmpfs and Btrfs are allowed as `development-unqualified`). Native OS Trash remains unsupported until WP09 passes. Bind displayed/runtime/package capability meanings consistently; a manifest must not claim cleanup disabled while its shipped binary offers it.

Do not label a profile certified before its later native checks pass. Existing permanent deletion is a development implementation; make release enablement explicit and tied to recorded qualification. Document any user-visible restriction this introduces. Avoid a large plugin framework or schema migration unless persisted capability data genuinely requires it.

Acceptance: unsupported cleanup is refused in backend and UI; enabled analysis still works. Tests distinguish Btrfs analysis acceptance from Btrfs deletion qualification and check manifest/runtime agreement.

Remaining:
- Show capability status in the TUI (for example, the Overview or help screen): permanent delete `development-unqualified`, OS Trash `unsupported`, and the filesystem of the current scan root. Add a render test.
- Confirm the macOS and Windows stub builds still compile (CI `Checks` workflow). Not verifiable locally.
- Decide how a later `qualified` state is recorded (for example, a reviewed constant per profile tied to `docs/linux-qualification.md`). Implement it only when WP05/WP12 evidence exists.

### WP03 — Record reproducible baseline

**Status: baseline commands recorded (2026-10-15).** All six baseline commands passed on Fedora 44, kernel 7.2.9, Btrfs repo, tmpfs `/tmp`.

Files: existing tests, `docs/linux-release-progress.md`, qualification fixtures/helpers under `tests/` or crate tests.

Run existing Rust, Node packaging and Linux PTY checks; separately run opt-in Btrfs fixtures. Record kernel, filesystem, architecture, Rust/Node versions and actual test skips. Check default and custom state directories, nonterminal invocation, raw names, exclusions, cancellation and last-valid scan retention. Do not count stub Windows/macOS compilation as Linux qualification or tests on tmpfs as Btrfs/ext4 evidence.

Acceptance: baseline commands/results and any unrelated failures are recorded before broader changes.

Remaining:
- Map each named check (default and custom state directory, nonterminal invocation, raw names, exclusions, cancellation, last-valid scan retention) to an existing test or a recorded manual run; add tests for any that are missing.
- Re-run the ignored Btrfs tests (`btrfs_root_and_state_are_scannable`, `btrfs_cli_profile_scans_and_previews_without_mutation`) after the owner removes the leftover `.btrfs-probe.B3NNXh/nested` subvolume.
- Run the same baseline on a real ext4 filesystem once the owner provides one.
- Decide whether the ignored `tui::tests::preview_screens` test should run in CI or stay manual.

## Stage 2 — Permanent-deletion authorization and recovery

### WP04 — Make confirmation exact and one-use

Files: `cleanup.rs`, `planning.rs` where shared semantics apply, `tui/mod.rs`, `tui/worker.rs`, cleanup/TUI tests.

Audit forged/deserialized plans, expired timestamps, replay, changed policy/catalog/scope, modified selections, and missing warnings. Bind authorization to the exact canonical plan and applicable scope/versions; do not trust a caller-supplied fingerprint paired with modified items. Require warning acknowledgement for that same plan. Consume authorization once, prevent concurrent reuse, and keep Cancel the default. A second attempt requires fresh review. Choose a narrow session/backend mechanism rather than persisting reusable approval.

Acceptance: altered, expired, future-dated, replayed or unacknowledged-warning plans cause zero mutations; legitimate isolated fixture operations remain possible on qualified profiles.

Added (2026-10-15):
- **Auto-filled items.** Scans add suggestions to OMG Trash (`auto_trash` is on by default), and `D` deletes the whole list after one confirmation. A user can delete items they never picked. Owner decision pending: turn auto-fill off by default, or require a separate confirmation for auto-filled items. Either way, the confirmation must list every item and show which ones were auto-filled.
- **Done 2026-10-15:** the backend holds each issued plan (`Index.issued`, memory only). `delete_permanently` consumes it once and compares the caller's plan with the stored one, including items, warnings, denials and times. A reopened index cannot use a plan from an earlier session. A scan change after review is refused.
- **Done 2026-10-15:** future-dated and expired plans are refused (unit tests with an injected clock). Still open: the owner decision on auto-fill (below).

### WP05 — Prove target and descendant safety

Files: `cleanup.rs` Linux helpers, `platform.rs`, `policy.rs`, `tests/cleanup.rs` and deterministic race-test helpers.

Audit the gaps between stat/identity checks and unlink, between opening/checking a directory and traversing it, and between descendant protection checks and actual removal. Directory descriptors alone do not prove these gaps safe. Add deterministic test barriers at acquisition, post-validation and pre-mutation; introduce replaced files, symlinked parents, renamed directories, newly inserted protected children, foreign ownership, bind mounts/subvolumes and hard-link cases. Verify birth-time support and inode reuse; missing reliable identity evidence must not silently qualify a profile.

Retain no-follow/no-cross-boundary behavior and home/sensitive/root protections. Do not use `canonicalize` followed by pathname deletion as a fix. If Linux primitives cannot meet the stated threat model for a target/operation, deny or explicitly defer that capability instead of claiming race safety. Do not automatically classify a stale repository as expendable; preserve its explicit warning/policy contract.

Acceptance: protected/replaced/out-of-scope objects survive every controlled race fixture, or the affected action is disabled with a clear reason. Native ext4 and Btrfs cases include selected subvolumes, nested subvolume denial, reflinks, sparse/compressed data where available, and snapshot-related size uncertainty.

Added (2026-10-15):
- **Birth-time fallback.** `cleanup.rs` compares `statx` birth time only when the filesystem reports it; otherwise revalidation is identity-only. The `Ext` magic also covers ext2, which has no birth time. Decide per target: a target without birth time is either denied on a qualified profile or explicitly listed as a residual risk. Test inode reuse on ext4, tmpfs and Btrfs.
- Single-file removal is still path-based after the checks (Linux has no unlink-by-descriptor). Document whether `unlinkat` on the verified parent descriptor closes the gap enough, or record it as a residual risk.
- Needs a real ext4 filesystem (owner-provided) and a Btrfs fixture whose cleanup is known to work.

### WP06 — Durable per-target outcomes and cancellation

Files: `cleanup.rs`, `index.rs` migrations, operation-history APIs/CLI/TUI, cleanup/recovery tests.

Record durable per-target intent before mutation and known outcome after it, with transactional database updates where possible. Exercise process termination and injected I/O/database failures at each boundary. Filesystem mutation and SQLite cannot be one atomic transaction; expose unknown/interrupted outcomes truthfully and never replay deletion or infer success merely from a missing path.

Fix partial-result classification: current top-level outcome selection can report `skipped` even when descendants were already removed. Handle failure of final rmdir or index updates after partial removal without losing that fact. Add bounded cancellation checks during traversal/removal; cancellation reports completed/partial/unattempted targets and does not undo completed deletion. Preserve recovery records and privacy-safe diagnostics.

Acceptance: interrupted/partial/failing attempts have truthful per-target outcomes visible after restart; intent-write failure causes zero mutations; no replay occurs. Cancellation stops new work within a measured bound.

Added (2026-10-15):
- Report observed free-space change separately from estimated bytes (S11). Already partly present (`observed_free_change`); verify it in outcomes and history.
- `omg history` already lists previews and delete operations as raw JSON (last 100). Add readable per-target outcomes in the CLI and the TUI.

## Stage 3 — Inventory, policy, and recoverable Trash

### WP07 — Correct inventory after mutation and failures

Files: `cleanup.rs::forget`, `rollup.rs`, `scanner.rs`, `index.rs`, scanner/rollup/cleanup tests.

Verify deletion updates against a fresh scoped metadata oracle, including hard links attributed to another path, overlapping selections, partial subtree removal, exclusions and changing files. The current subtract-only rollup path needs evidence that deleting one hard-link name does not corrupt global or ancestor totals. Recompute/reconcile affected scopes when exact adjustment is unavailable; mark cached coverage stale until reconciled rather than fabricate exact totals.

Add process-kill and disk-full/permission/corruption tests for scan publication and migrations. Preserve last-valid results, never overwrite unreadable/corrupt state as a repair shortcut, and document consistent backup/recovery of DB/WAL/SHM. Distinguish allocated inventory from volume availability and reclaimable bytes; external disk activity can affect observed free-space deltas.

Acceptance: after successful and partial operations, refreshed counts/totals agree with the independent oracle; no failed staging scan replaces a valid published generation; database recovery preserves user settings/history where readable.

Added (2026-10-15, from Phase 1 open gates):
- Barrier-controlled concurrent change and mount-substitution tests *during scanning* (not only during deletion).
- Real process interruption of a running scan, disk-full and database fault injection, and migration behavior for a future schema version.
- Recovery tooling and instructions for a damaged state directory (read-only diagnosis stays in `omg doctor`).

### WP08 — Qualify findings and scanner scaling

Files: `catalog.rs`, bundled catalog, `policy.rs`, `scanner.rs`, `rollup.rs`, service scheduling, relevant fixtures/evidence.

Test real supported npm/pip/uv and stale dependency layouts, custom roots, active tool usage, protected near-misses and dismissed suggestions. Record supported versions and uncertainty; no rule's label authorizes deletion. Ensure installed tools are not suggested as project dependencies. Provide full-inventory search/grouping where page-level queries currently hide findings; inspect the current behavior before implementing.

Measure scan/refresh/cancel/peak memory/FD/backlog/SQLite growth on representative trees. Verify overlapping-root/mount alias accounting or explicitly keep one-root semantics and show scope limits. Fix observed resource regressions with measured limits. True incremental scanning is a follow-up optimization unless required to meet the selected budgets: reliable full reconciliation is an acceptable first-release implementation if accurately documented. If incremental reuse is added, watcher overflow, restart gaps and missed events force reconciliation; directory mtime alone never proves descendants unchanged.

Acceptance: findings are explainable with real positive/negative fixtures, large trees remain within recorded budgets, and every performance/support claim is backed by the measured profile. No hidden invented budget or whole-disk coverage claim.

Added (2026-10-15, from checklist V05/V06 and Phase 1–2 gates):
- Tool-root overrides (custom npm/pip/uv cache locations) — not implemented yet.
- Representative cold, warm and refresh benchmarks with RSS, file-descriptor and queue measurements; adaptation to I/O pressure.
- Cached status identity unions are computed from SQLite on demand; measure them and optimize only with a tested consistency check.
- Validate directory rollups on representative large trees.
- Full category aggregation and global search in Findings.

### WP09 — Investigate and qualify Linux OS Trash separately

Files: new narrowly scoped Linux Trash adapter, `planning.rs`, `cleanup.rs` integration, TUI actions, audit/history, isolated native fixtures, relevant docs.

Research the current official freedesktop Trash specification and chosen maintained library/native implementation before implementing it. Design separate reversible Trash versus permanent-delete actions, previews and capability gates. Verify same-volume and other supported mount layouts, private Trash permissions, original-path metadata, restore usability, collisions, unavailable/read-only Trash and raw filenames. No permanent-delete fallback on Trash failure.

Resolve stable acquisition/claim semantics before moving user data. Staging/rename can mutate a substituted object and is not automatically an identity-safe solution. Test substitutions, descendant writers, partial moves and crashes with durable recovery records. Never overwrite an original-location replacement during recovery. Never auto-empty Trash; moving to Trash does not promise freed disk space.

Acceptance: enable only target/environment combinations whose safety and restore semantics have real evidence. If the race/claim problem remains unresolved, deliver the documented investigation and keep OS Trash disabled; an analysis-only release can proceed, but a full cleanup release cannot claim this work complete.

Added (2026-10-15):
- Internet research goes through the `agent-reach` skill (owner's global rule).
- The capability contract already has `os_trash()`; flip it only for evidenced profiles, and add the manifest field at the same time (`assemble-release.js`, `verify.js`, `release.test.js`).
- Note the naming clash: "OMG Trash" (saved list) versus OS Trash. The UI must keep them clearly different (WP10).

## Stage 4 — UI, service, and native release qualification

### WP10 — Clear action and outcome UI

Files: `tui/mod.rs`, `render.rs`, `worker.rs`, CLI command definitions, PTY/render/event tests.

Make saved-path membership, OS Trash and irreversible deletion distinct. Preview all affected targets/warnings with scrolling; large plans must not authorize unseen items. Display partial/unknown/failed outcomes and recovery access. Audit hostile names, narrow screens, keyboard events, resize, panic/interrupt terminal restoration, stale result refresh and nonterminal errors. File-manager opening uses fixed argv/native behavior, not shell evaluation.

Acceptance: meaningful event-sequence and PTY tests show exact confirmation, usable cancel/recovery, escaped filenames and restored terminal settings; actual Linux user sessions identify remaining usability defects.

Added (2026-10-15):
- **`DeleteNow` staging bug:** it adds the item to OMG Trash, then prepares the plan. If preparation fails, the item stays in the list and the list does not refresh (`refresh` is only set on success). Fix by undoing the staged mark on failure, or by preparing before marking.
- Mark auto-filled items visibly in the OMG Trash tab, and show the auto-fill setting there.
- Capability status in the TUI, if not done in WP02.
- Native check of the fixed `/usr/bin/xdg-open` location-open action in a real desktop session.
- Independent Linux developer usability review (checklist V01, Linux part).
- Wording: remove leftover help text that describes cleanup in older terms.

### WP11 — Qualify optional scan-only service

Files: `service/*`, `omg-cli/src/svc.rs`, service configuration/units and tests.

Verify install/start/stop/uninstall, login/restart, moved/upgraded binaries, orphaned units, foreground scan handoff, battery/backoff behavior, state/socket substitution and notification privacy on native systemd Linux. Keep cleanup absent from IPC and scheduling. Test WSL separately with and without systemd user-manager support. Measure idle/scan CPU, memory and I/O; apply measured resource limits.

Acceptance: no implicit installation or cleanup; foreign units are preserved; unsupported sessions fail clearly; repeated lifecycle and handoff tests pass on the advertised profiles.

Added (2026-10-15, from Phase 5 open gates):
- `notify-send` delivery in a real desktop session.
- Rejection of a *different* uid over IPC (`SO_PEERCRED`), which needs a second local account (owner-provided).
- journald log-size policy for the service.
- Upgrade of a registered binary path: today `doctor` detects it and only `omg service install` repairs it. Decide whether that is enough and document it.
- Idle-budget measurement on representative machines.

### WP12 — Publish a tested compatibility matrix

Files: `docs/linux-qualification.md`, release docs, platform/capability gates and CI matrix.

Record separate analysis/permanent-delete/OS-Trash/service evidence for real ext4, Btrfs and supported WSL2 ext4 setups, plus tmpfs where appropriate. Cover oldest supported kernel/openat2, glibc baseline, x86_64 CPU assumptions, filesystem/mount options, private state permissions, disk-full/read-only conditions and cloud/network denial. Do not claim all Linux distributions; state tested configurations and minimum requirements. Test GNU ARM64 natively before adding it. WSL host virtual-disk reclamation is explanatory only, never an automatic privileged operation.

Acceptance: every enabled capability/target has reproducible native results; unavailable hosts remain unverified and absent from supported release claims.

Added (2026-10-15):
- Native tests that FUSE, cloud and network mounts (for example sshfs, rclone, gvfs) are refused (checklist V02–V03, Linux part).
- Raw (non-UTF-8) path handling on each profile.
- Record which profiles have birth time (feeds WP05).
- Needs an ext4 host and a WSL2 host (owner-provided).

## Stage 5 — Distribution and release

### WP13 — Build and install reproducible artifacts

Files: `.github/workflows/check.yml`, `artifacts.yml`, `scripts/assemble-release.js`, `verify-release.js`, package launcher/tests and release docs.

Audit CI least privileges, pin/review actions and build/runtime baselines, and build every supported target on an appropriate environment. Preserve Windows/macOS stub builds as compilation checks only. Test ordinary npm, offline/manual binaries, paths with spaces, missing optional packages, no-Rust/no-compiler end-user installs, argv/signals/exit status, minimum runtime dependencies, and safe refusal to overwrite unrelated executables. End users installing standalone verified binaries need no Node; the npm launcher requires its documented Node baseline.

Acceptance: clean supported machines install/run/uninstall verified matching-version artifacts with state preserved; unsupported libc/architectures fail clearly; upgrades do not break registered services silently.

Added (2026-10-15, from Phase 4 open gates):
- Run `artifacts.yml` for real (it has never run).
- Install-directory ancestor redirection and time-of-check/time-of-use hardening between verification and execution in the launcher.
- Clean-machine runtime ABI (glibc) matrix.

### WP14 — Upgrade, trust, and release governance

Files: `index.rs` migrations, release scripts/trust/manifest validation, `scripts/sbom.js`, release docs/tests.

Test migrations from every supported historical schema, consistent backup, interruption, wrong/future schema and supported rollback behavior. Prepare reviewed signing/publisher configuration, artifact provenance, dependency advisory/license review, schema-validated SBOM/notices, authenticated key fingerprints, rotation/revocation and offline trust instructions. Use synthetic keys for tests; production private keys never enter the repo or tool output. Align runtime/manifest capability claims established by WP02.

Acceptance: upgrade/recovery and tamper/target/version/signature checks pass; actual supported package contents are reviewed. External publisher ownership and production trust inputs are explicit remaining prerequisites if not provisioned.

Added (2026-10-15, from Phase 2 and Phase 4 open gates):
- Catalog signing key governance: provisioning, rotation, and re-verification of stored signed catalogs after local-state tampering. Today, catalog import is disabled until an owner-held key exists.
- Publish a security reporting channel and the adopted license/notice files.

### WP15 — Assemble the release decision

Files: release checklist, compatibility matrix, current evidence/progress, release notes and security-reporting documentation.

Run final required checks on final artifacts, not only debug builds. Prepare a concrete release candidate with exact version, digests, support matrix, verified features, known limitations and upgrade/uninstall guidance. Audit privacy and require independent developer review. Decide between an analysis-only beta and qualified cleanup release from actual gate results; unresolved cleanup safety means that capability is disabled in the shipped binary, not merely disclaimed in prose.

Acceptance: every advertised feature meets its gate or is explicitly unsupported/deferred; all documentation/package/runtime claims agree. Public publishing happens only with the necessary explicit authorization and supplied production identities. Do not call a prepared unpublished candidate a published release.

Added (2026-10-15):
- Diagnostic privacy review: notifications, `doctor` output and logs contain no paths or filenames by default.
- Document the threat-model limit: OMG does not defend against a hostile process running as the same user replacing its state or binary.
- Reconcile `docs/checklist.md`: tick only the Linux items whose evidence passed, and leave non-Linux items open.
- Owner decision: the release ships permanent delete as `development-unqualified`, or disables it in the shipped binary. Prose disclaimers alone are not enough.

## Owner and environment prerequisites

| Item | Needed from | Blocks |
| --- | --- | --- |
| Auto-fill default: off, or a separate confirmation for auto-filled items | Owner | WP04 |
| Remove leftover Btrfs subvolume `.btrfs-probe.B3NNXh/nested` (`sudo btrfs subvolume delete`, then `rmdir`) | Owner | WP03 Btrfs re-run, WP05 Btrfs fixtures |
| A real ext4 filesystem for tests (spare partition or owner-mounted loop image) | Owner | WP03, WP05, WP12 |
| A second local user account | Owner | WP11 different-uid IPC test |
| A desktop session for `notify-send` and `xdg-open` checks | Owner | WP10, WP11 |
| WSL2 host | Owner | WP11, WP12 WSL profile |
| GNU ARM64 machine (optional) | Owner | WP12, WP13 ARM64 target |
| Remote CI runs (`Checks`, `artifacts.yml`) | Owner (push access) | WP02 non-Linux check, WP13 |
| Production release signing key, publisher identity, npm namespace | Owner | WP13–WP15 |
| Authority to publish | Owner | WP15; never without explicit approval |
| Release decision: ship permanent delete as unqualified or disable it | Owner | WP15 |

## Check commands and native evidence

Baseline commands (adapt only when justified by the final implementation):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build -p omg-cli --locked
python3 -m unittest discover -s tests -p 'test_*.py' -v
node --test packages/oh-my-gigabytes/test/release.test.js
```

Run the existing ignored Btrfs scan/state and CLI tests explicitly with `OMG_BTRFS_TEST_DIR` on a verified Btrfs fixture parent. Add separately scoped opt-in native cleanup/race/failure tests; do not make ordinary tests destructively use that environment path itself. Native ext4, Btrfs, WSL2, clean-install and production-runtime results must be recorded independently. A skipped test, mock, compilation result or invented screenshot does not establish native behavior.

## Deferred features and other platforms

Not required for the Linux release; keep them in a backlog rather than expanding this safety-critical scope:

- Duplicate content analysis and Git-aware analysis (checklist Phase 5, deliberately not delivered).
- Advisory filesystem watchers and dirty-scope invalidation, and true incremental scanning (full reconciliation is the documented baseline).
- Multi-root and per-volume status, beyond showing the one-root scope limit (WP08).
- AI ranking and personalization (checklist Phase 6).
- Broad tool cleanup adapters and automatic privileged maintenance.
- Developer catalog expansion driven by user feedback.

After Linux qualification, create separate native Windows and macOS plans reusing core inventory/policy/UI contracts but implementing their own path/file identity, permissions, cloud placeholder, Trash, state, terminal, service and installer semantics. WSL is not the Windows adapter. Do not enable those adapters by changing a platform boolean.

## Copyable small-agent assignment

> Read `docs/linux-release-implementation-plan.md` and `docs/linux-release-progress.md`. Implement the next unfinished work package only, including the "Remaining" and "Added" items under it, and its meaningful verification. Work and commit on `main`; do not create branches or push. Preserve others' edits. Record changed files, exact checks, actual native results/skips, unresolved conditions and next work package in the progress document, and update the matching phase evidence file. Do not claim safety or qualification beyond evidence, publish anything, provision production secrets, run destructive tests on real user data, or enable an unqualified capability. If a required environment or owner decision is unavailable, finish the independent work and clearly record the remaining gate. End with a concise report suitable for the next agent session.
