# Phase checklist

Status is evidence-based. `[x]` means the stated deliverable exists and its required local check passed; `[ ]` means pending. A capability denial completes a design decision, not platform certification. See [phase0.md](phase0.md) for the current evidence and blockers.

## Phase 0 — Competitive validation and safety design

- [x] Preserve the complete product brief and document implementation decisions.
- [x] Research current competing tools, maintenance and component licenses with sources.
- [x] Define differentiation: explanations/safety plus fast, coverage-aware cached startup.
- [x] Record actual founder feedback with provenance and limitations.
- [ ] Validate the four success questions and repeat-use value through developer prototype sessions.
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
- [ ] **Phase 0 exit:** actual developer prototype feedback validates the differentiation hypothesis; all design/evidence gates reviewed.

**Current conclusion:** local engineering/design deliverables verified; full Phase 0 exit remains blocked by developer prototype validation. No deletion, ordinary scan, platform certification or benchmark performance claim is enabled by the probe.

## Phase 1 — Scanner and index

- [ ] Create Rust workspace, pinned toolchain/MSRV and owner-approved license.
- [ ] Admit a maintained, resource-bounded walker with source/native tests.
- [ ] Implement native metadata, roots, exclusions, cloud denial and volume samples.
- [ ] Implement metadata-only bounded traversal, progress and cancellation.
- [ ] Implement SQLite schema, migrations, staging publication and coverage.
- [ ] Implement singleton scan ownership and interruption/corruption recovery.
- [ ] Verify apparent/allocation, hard-link/root/mount alias accounting.
- [ ] Implement incremental invalidation and reliable full/gap reconciliation.
- [ ] Deliver scan/status/doctor and read-only cached TUI.
- [ ] Record representative cold/warm/refresh/resource measurements.
- [ ] Pass Phase 1 analysis-only acceptance gates.

## Phase 2 — Catalog and policy

- [ ] Implement strict bounded TOML and builtin discovery/action schema.
- [ ] Implement evidence fingerprints, effective policy tiers and exclusions.
- [ ] Author independently sourced developer rules with negative fixtures.
- [ ] Deduplicate candidate targets/estimates and implement scoped ignores.
- [ ] Ship known-good catalog and authenticated import/rollback lifecycle.
- [ ] Verify invalid/tampered catalogs cannot activate or grant shell execution.
- [ ] Pass Phase 2 traceable-recommendation gates.

## Phase 3 — TUI and cleanup

- [ ] Deliver keyboard navigation/details/search/filter/ignore/history/location opening.
- [ ] Deliver exact immutable preview, cancel-default separate confirmation and explicit dry-run.
- [ ] Prove native target acquisition and Trash race semantics for each enabled capability.
- [ ] Implement authoritative session-bound authorization and operation locks.
- [ ] Implement durable intent, per-target outcomes and interrupted reconciliation.
- [ ] Verify cancellation, stale/replaced/protected targets and no permanent fallback.
- [ ] Verify terminal restoration, narrow screens and truthful processed/space-delta reporting.
- [ ] Pass Phase 3 safety gates before enabling any cleanup.

## Phase 4 — Cross-platform release

- [ ] Establish native analysis/Trash certification and final OS/filesystem/architecture matrix.
- [ ] Publish security reporting channel and adopted license/notices.
- [ ] Build precompiled native artifacts and exact-version npm launcher/platform packages.
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
