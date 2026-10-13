# Phased roadmap and acceptance gates

Status: 2026-10-13. **Phase 0 research/design is complete under owner-approved scope revision D14.** Additional evidence and deferred native/user-validation gates are recorded in [phase0-evidence-review.md](phase0-evidence-review.md); available spike results are in [phase0.md](phase0.md). Phases 1–6 remain pending. Track every phase in [checklist.md](checklist.md).

This follows the supplied phases 0–6 while adding concrete dependencies and evidence. Use gates rather than arbitrary dates or universal scan-time promises. Work packages: [plans.md](plans.md). Verification: [testing.md](testing.md).

## Phase overview

| Phase | Outcome | Destructive capability | Depends on |
| --- | --- | --- | --- |
| 0 | Validated differentiation, safety contracts, platform feasibility | None | Product brief and research |
| 1 | Bounded local metadata inventory and honest accounting | None | Phase 0 scope/metadata contracts |
| 2 | Traceable recommendations and validated TOML catalog | None | Phase 1 inventory; core policy |
| 3 | Keyboard-first review and certified Trash operations | Explicit approved plans only | Phase 2; native race-safety gate |
| 4 | Cross-platform supported release and authenticated distribution | Only certified adapters | Phases 1–3 test evidence per target |
| 5 | Optional resource-conscious background intelligence | No service cleanup | Reliable foreground release/coordination |
| 6 | Optional local Review ranking, if justified | Never model-authorized | Consented useful local feedback and evaluation |

Cross-platform metadata/safety spikes begin in Phase 0/1, not only in Phase 4. A read-only TUI may begin in Phase 1; full interactions belong to Phase 3. Service seams/native feasibility can be reviewed in Phase 4, with actual optional service delivery in Phase 5.

## Phase 0 — Competitive validation and safety design

### Deliverables

- Source-cited comparison against current `dua`, `dust`, Czkawka/Krokiet, BleachBit, and later duplicate-analysis candidates.
- Source-based workflow/constraint review and a ready-to-run prototype comparison protocol; independent developer sessions deferred under D14 to Phase 3 usability acceptance.
- Mandatory safety invariants S01–S12, threat model, protected scope, and denial semantics.
- Proposed OS/filesystem/architecture capability matrix with analysis, cleanup, and service certification separate.
- Native metadata/cloud/root-boundary spike and a plan for identity-safe Trash acquisition on each target.
- Dependency/license strategy, source provenance policy, and initial benchmark fixture design.

### Exit gate

- A clear provisional differentiation hypothesis has sourced workflow evidence and recorded founder feedback; unavailable independent prototype sessions have explicit later gates under D14, without claims of majority/validated usefulness.
- Default roots/exclusions, metadata-only behavior, repository protection, estimates, and confirmation rules are explicit.
- Native feasibility risks have executable spike results or documented capability denials; no unsolved behavior is advertised as supported.
- Adversarial suite design covers replacements, protected descendants, cloud virtual directories, crash windows, and release tampering.

### Evidence artifacts

Updated `research.md`, `plans.md`, `safety.md`, and `phase0.md` supply design/research, founder preference provenance, capability denials and verified synthetic Linux spike results. `phase0-validation.md` defines the pending prototype-feedback and benchmark-fixture protocol. Founder preference is not completed prototype validation.

`phase0-evidence-review.md` records the additional Linux/macOS source evidence, D14 closure and V01–V06 deferred register. This changes Phase 0 scheduling only; mandatory native safety/reliability/release criteria are binding.

## Phase 1 — Scanner and local index

### Work sequence

1. WP01: Rust workspace, toolchain/license decision, typed model, native CI baseline.
2. WP02/WP03: platform metadata and walker admission; choose maintained bounded traversal.
3. WP04: SQLite migrations, private state, scan ownership, staging/publication, coverage.
4. WP05: CLI scan/status/doctor, read-only TUI, background foreground-refresh worker, benchmarks.
5. Incremental dirty-scope refresh plus periodic and restart-gap reconciliation. Watcher optimization is optional; truthful full enumeration is the baseline.

### Required behaviors

- Metadata-only ordinary scanning; no hydration/content hashing; explicit supported local roots.
- Bounded memory/workers/queues/descriptors, cancellation under backpressure, progress and typed access errors.
- Allocated/apparent sizes with unknown/shared limits; hard-link/mount alias/overlapping root accounting.
- Last valid index on startup; independent volume samples and scoped inventory.
- Interrupted scans cannot publish complete coverage; successful-parent enumeration required before pruning missing children.
- Patched bundled SQLite, safe migrations, corruption/disk-full recovery, singleton scan ownership.

### Exit gate

**Reliable, explainable storage analysis with no deletion capability.**

- Metadata oracle agrees on fixture inventories/accounting within documented allocation limits.
- Cloud/redirection/access/error/cancellation tests pass on environments claimed for analysis.
- Crash tests preserve prior valid snapshots and mark interrupted scopes honestly.
- Warm startup, cold scan, incremental/reconciliation refresh measurements are recorded with hardware/tree metadata and peak resources.
- The UI displays useful cached results before a full refresh finishes.
- No callable filesystem cleanup entry point is exposed by this milestone.

### Release option

An explicitly labelled analysis-only preview may be published for certified platforms if packaging/authenticity and privacy tests already pass. It is not the full cleaner release.

## Phase 2 — Rules catalog and safety tiers

### Deliverables

- Strict TOML schema, stable IDs/revisions, compatibility/action/discovery enums, bounded parsing.
- Independently authored small developer catalog; npm/pip inventory first; Review findings only where evidence and policy qualify.
- Traceable evidence/freshness, requested/effective tier separation, protected/unknown explanations.
- Cache invalidation by rule/catalog/policy/scope/evidence fingerprints.
- Overlap/hard-link-safe candidate totals, local exact/subtree ignores with review/removal.
- Bundled known-good catalog, authenticated offline import and last-good/rollback lifecycle; optional online fetch can follow later through the same verifier.

### Exit gate

**Every candidate has traceable evidence, an appropriate tier, and a tested policy decision.**

- Positive/near-miss/protected/unknown-version/incomplete fixtures pass for every activated rule.
- No filename/age/size heuristic alone assigns Safe.
- Invalid or tampered updates cannot replace the active catalog; action IDs cannot request shell execution.
- Rule and ignore changes invalidate derived findings without selecting or authorizing cleanup.
- Unsupported categories remain clearly informational/deferred.
- No destructive feature yet; proposed action eligibility is not execution permission.

## Phase 3 — TUI and cleanup execution

### Subphase 3A: Review interface

- Navigation/grouping, details, search/tier/path/category/size filters, exact scoped ignore review, location opening, history.
- Nonoverlapping selection totals labelled estimates; Info/protected actions disabled.
- Immutable complete preview, separate confirmation with cancel default, nonterminal execution denied.
- Dry-run with explicit targets; no target mutations, utility execution, or staging.

### Subphase 3B: Native safety gate

- WP07: safe acquisition/claim semantics, protection of descendants/ancestors, native Trash outcomes.
- Barrier-controlled symlink/reparse/target/mount replacement tests, concurrent directory changes, unavailable Trash, and partial API failures.
- Durable intent/outcome with unresolved crash reconciliation; no automatic replay.
- Permission/cloud/unsupported environments fail closed without deletion.

This gate can block cleanup for a platform or target type while read-only features remain useful. “Revalidated immediately beforehand” alone is not enough.

### Subphase 3C: Explicit operations

- WP08: session-bound authorization, exclusive operation lock, postconfirmation revalidation, sequential handler execution.
- Per-target completed/skipped/failed/cancelled/unresolved outcomes, native receipts where possible.
- Available-space samples and truthful Trash-retention reporting.
- Safe cancellation boundaries, recovery guidance and no universal undo promise.

### Exit gate

**Users can understand, review, and safely execute supported cleanup operations without silent deletion.**

- S01–S12 tests pass for each enabled handler/environment.
- Protected descendants and replaced paths are never sent to Trash/utility execution.
- Selection/refresh/service/startup cannot trigger cleanup.
- Confirmation binds the full exact scope, warnings, versions, and target identity set.
- Dry-run makes no target changes; Trash failure never calls permanent-delete fallback.
- UI works at tested narrow/standard terminal sizes and restores terminal state on interruption.
- Result totals distinguish attempted/processed/observed space change accurately.
- V01: independent Linux/macOS developer review records comprehension, comparison and contrary findings before usability/validated-usefulness claims; Phase 0 desk research does not satisfy this gate.

## Phase 4 — Cross-platform hardening and distribution

### Deliverables

- Native Windows/macOS/Linux integration suites for metadata, policy, Trash, raw paths, permissions, and cloud provider exclusions.
- Final supported OS versions/filesystems/architectures/libc/terminal matrix with per-feature capabilities.
- npm wrapper with same-version platform packages, signed binary manifest/digests, build provenance, SBOM/notices.
- Manual/offline installation, clean-machine install/upgrade/uninstall, `omg doctor`, state migrations and recovery procedure.
- Code signing/notarization where adopted, tested installation failure messages and architecture/runtime handling.
- Security reporting channel, threat model, release checklist, dependency review.
- Service native integration design tested for feasibility; no automatic registration.

### Exit gate

**All mandatory safety and reliability tests pass on each officially supported platform.**

- Real native runs establish capabilities; cross-compilation/emulation alone does not certify filesystem/Trash behavior.
- A clean machine needs no Rust/compiler toolchain to install and run the packaged binary.
- Signature/digest/target/version mismatch prevents activation/execution.
- Offline manual artifacts can be authenticated without contacting the catalog service.
- `--ignore-scripts` install works for the preferred no-downloader package design; omitted platform packages fail usefully.
- Upgrade and corruption scenarios preserve truth/preferences/history, with older unsupported clients rejected clearly.
- Published features and operating-system support match evidence rather than the full future roadmap.

## Phase 5 — Background service and proactive intelligence

### Deliverables

- Explicit per-user service install/start/stop/status/uninstall on supported systemd/launchd/Task Scheduler environments.
- Scan-only service API, native low priority, bounded resources, heartbeat, backoff/cancellation and foreground handoff.
- Periodic reconciliation, advisory invalidations, dropped-event/restart recovery.
- Optional low-free-space threshold notifications with hysteresis, dwell/cooldown and privacy-safe wording.
- Bounded local volume/growth trend retention and explainable observed change.
- Developer catalog expansion based on volunteered feedback.
- Optional duplicates and Git-aware analysis as separately gated features, not prerequisites for reliable service delivery.

### Exit gate

**The service improves decisions without unattended cleanup or continuous resource burden.**

- Service cannot invoke cleanup even with forged IPC/config/catalog input.
- Concurrent foreground/service scans share ownership without duplication/corruption.
- Idle/scan/battery/failure/backoff resource measurements meet documented machine-specific budgets.
- Start/stop/uninstall and missing user-manager/session environments report truthfully.
- Threshold oscillation does not spam notifications; notification text does not expose sensitive filenames.
- Duplicate analysis, if delivered, is explicit opt-in content I/O with protected/cloud exclusions, bounded hashing, replacement checks and Review defaults.
- Git-aware support, if delivered, never treats ignored/clean/pushed state as proof that repository data is disposable.

## Phase 6 — Optional local personalization

### Prerequisites

- Demonstrated ranking problem not solved by deterministic category/size filters.
- Explicit consent for local preference use and enough meaningful feedback for evaluation.
- Local-only data lifecycle, user disable/reset controls, reproducible evaluation fixtures.

### Exit gate

- Ranking improves user decision usefulness relative to a deterministic baseline.
- Model only orders Review recommendations; cannot authorize actions, change mandatory protection, or promote unsafe items.
- No telemetry, model-training upload, or hidden network dependency.
- Rule explanations/evidence remain primary and inspectable.

This phase is optional and may never be justified. No model is part of MVP architecture/dependencies.

## Gate evidence tracking template

For each phase/adapter/rule capability, record:

```text
Capability / phase:
Build + catalog + policy versions:
OS / filesystem / architecture / terminal:
Fixtures and commands:
Results and known limitations:
Benchmark methodology + resource peaks:
Reviewer / date:
Accepted, blocked, or deferred:
Documentation updated:
```

Do not mark acceptance by TODO completion alone. Failing safety gates block the relevant action feature; unresolved platform behavior must appear in the release capability matrix.
