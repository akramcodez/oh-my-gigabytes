# Phase checklist

Status is evidence-based. `[x]` means the stated deliverable exists and its required local check passed; `[ ]` means pending. A capability denial completes a design decision, not platform certification. See [phase0.md](phase0.md) for the current evidence and blockers.

Phase 0 is **complete under revised scope D14**. Deferred checks are shown separately rather than checked as passed; [phase0-evidence-review.md](phase0-evidence-review.md) defines their later gates.

## Phase 0 — Competitive validation and safety design

- [x] Preserve the complete product brief and document implementation decisions.
- [x] Research current competing tools, maintenance and component licenses with sources.
- [x] Define differentiation: explanations/safety plus fast, coverage-aware cached startup.
- [x] Record broad Linux/macOS audience priority; keep Windows/WSL as an additional platform track.
- [x] Record actual founder feedback with provenance and limitations.
- [x] Record founder workflow and authorized scoped metadata observations, including distinct Windows host/WSL capacity.
- [x] Research official Linux/macOS/Windows workflows without claiming majority preference.
- [x] Provide a read-only synthetic prototype and comprehension/usefulness questions.
- [x] Complete additional source-based Linux/macOS evidence review and record limitations.
- [x] Record owner-approved deferral of unavailable developer sessions with later acceptance gates.
- [x] Define mandatory invariants S01–S12 and an adversarial test plan.
- [x] Define roots, exclusions, repository protection, tier and confirmation semantics.
- [x] Separate analysis, cleanup and service capabilities for each target environment.
- [x] Build an executable metadata/root-redirection Linux feasibility probe.
- [x] Verify Linux no-follow acquisition, hard-link identity, sparse allocation and subtree timestamp counterexample using synthetic fixtures.
- [x] Provide fail-closed capability denials for unproven cloud, Trash, Windows and macOS behavior.
- [x] Document native identity-safe Trash investigation and crash/outcome requirements.
- [x] Define dependency/license admission and release provenance strategy.
- [x] Define representative benchmark fixtures and evidence recording format.
- [x] Run and record local probe/tests and documentation checks.
- [x] **Phase 0 exit (D14):** research/design evidence reviewed, provisional differentiation explicit, available spike verified and deferred validation assigned.

**Current conclusion:** Phase 0 research/design complete; Phase 1 may begin. No deletion, ordinary scan, platform certification or benchmark performance claim is enabled by closure.

### Deferred work — not passed

- [ ] V01: independent Linux/macOS developer comprehension and competitor comparison before Phase 3 usability acceptance.
- [ ] V02–V03: native metadata/cloud/raw-path tests before the respective analysis capability is certified.
- [ ] V04: native Trash/race/recovery tests before any action is enabled.
- [ ] V05: representative benchmarks before performance targets/release claims.
- [ ] V06: real tool/version/fixture qualification before rule/action activation.

## Phase 1 — Scanner and index

- [x] Create Rust workspace, pinned toolchain/MSRV and owner-approved license.
- [x] Implement OMG-owned Linux worker pool with bounded events and SQLite-backed work queue; verify synthetic integration fixtures.
- [ ] Implement native metadata, roots, exclusions, cloud denial and volume samples.
- [x] Implement Linux metadata-only bounded traversal, progress and cancellation.
- [x] Implement initial SQLite versioned schema, staging publication and coverage.
- [ ] Implement singleton scan ownership and interruption/corruption recovery.
- [ ] Verify apparent/allocation, hard-link/root/mount alias accounting.
- [ ] Implement incremental invalidation and reliable full/gap reconciliation.
- [x] Deliver scan/status/doctor and initial read-only cached TUI.
- [ ] Record representative cold/warm/refresh/resource measurements.
- [ ] Pass Phase 1 analysis-only acceptance gates.

See [phase1-evidence.md](phase1-evidence.md) for implemented Linux development scope, six passing tests, synthetic benchmark and outstanding full-acceptance gates.

## Phase 2 — Catalog and policy

- [x] Implement strict bounded TOML and inventory-only builtin discovery/action schema.
- [ ] Implement evidence fingerprints, effective policy tiers and exclusions.
- [ ] Author independently sourced developer rules with negative fixtures.
- [ ] Deduplicate candidate targets/estimates and implement scoped ignores.
- [ ] Ship known-good catalog and authenticated import/rollback lifecycle.
- [x] Verify invalid/tampered catalogs cannot activate or grant shell execution in synthetic signed update tests.
- [ ] Pass Phase 2 traceable-recommendation gates.

See [phase2-evidence.md](phase2-evidence.md): bundled inventory rules, policy, fingerprints, scoped ignores, signature/replay/rollback tests implemented; real tool/native qualification and release trust provisioning remain pending.

## Phase 3 — TUI and cleanup

- [ ] Deliver keyboard navigation/details/search/filter/ignore/history/location opening.
- [ ] Deliver exact immutable preview, cancel-default separate confirmation and explicit dry-run.
- [ ] Prove native target acquisition and Trash race semantics for each enabled capability.
- [ ] Implement authoritative session-bound authorization and operation locks.
- [ ] Implement durable intent, per-target outcomes and interrupted reconciliation.
- [ ] Verify cancellation, stale/replaced/protected targets and no permanent fallback.
- [ ] Verify terminal restoration, narrow screens and truthful processed/space-delta reporting.
- [ ] Pass Phase 3 safety gates before enabling any cleanup.

See [phase3-evidence.md](phase3-evidence.md): read-only navigation/details/filters/ignores, exact explicit dry-run, revalidation/history and 15 passing Rust tests plus PTY smoke implemented. Actual Trash, native safety, operation outcomes and independent usability acceptance remain blocked; cleanup is disabled.

## Phase 4 — Cross-platform release

In progress: [phase4-evidence.md](phase4-evidence.md) and [release.md](release.md) record local signed packaging/verification, offline install tests and blocked cross-target qualification.

- [ ] Establish native analysis/Trash certification and final OS/filesystem/architecture matrix.
- [ ] Publish security reporting channel and adopted license/notices.
- [ ] Build precompiled native artifacts and exact-version npm launcher/platform packages.
- [x] Implement private launcher/platform/offline package assembly with no install-time downloader or compiler.
- [x] Implement strict signature/digest/version/target verification with synthetic tamper tests.
- [x] Verify isolated Linux offline npm install/reinstall/uninstall with scripts disabled and state preserved.
- [ ] Implement and test signed manifest/digest/target/version verification and provenance.
- [ ] Test clean installation, upgrade, offline/manual path and uninstall.
- [ ] Verify migration/recovery/runtime/terminal behavior on supported targets.
- [ ] Pass complete release checklist with build-specific evidence.

## Phase 5 — Optional service and intelligence

- [ ] Deliver explicit per-user install/start/stop/status/uninstall integrations.
- [ ] Implement authenticated scan-only coordination and foreground priority.
- [ ] Verify native resource limits, backoff, cancellation and restart-gap reconciliation.
- [ ] Implement opt-in privacy-safe threshold notifications with hysteresis/cooldown.
- [ ] Add bounded local growth trends and evidence-driven developer coverage.
- [ ] Gate optional duplicate content analysis and Git-aware analysis independently.
- [ ] Pass service acceptance; no service cleanup entry point.

## Phase 6 — Optional personalization

- [ ] Demonstrate a ranking need and collect explicitly consented local preferences.
- [ ] Evaluate local Review-only ranking against deterministic baseline.
- [ ] Prove no policy override, tier promotion, authorization or upload behavior.
- [ ] Deliver disable/reset controls, only if justified.
