# OMG documentation

**OMG (Oh My Gigabytes)** is a local-first, keyboard-first storage intelligence application for developers. A Rust engine scans filesystem metadata into SQLite, a versioned catalog explains possible cleanup, and an authoritative policy engine controls every operation. Distribution uses the npm package `oh-my-gigabytes`; the executable is always `omg`.

**Product principle:** Find it, explain it, verify it, and let the user decide.

## Status and reading order

Documentation baseline: **2026-10-13**. Rust development milestones now provide scoped Linux metadata scanning/SQLite, inventory catalog/policy, read-only TUI, explicit dry-run and preview history. Full phase acceptance and native release certification remain incomplete. Current evidence: [phase1-evidence.md](phase1-evidence.md), [phase2-evidence.md](phase2-evidence.md), [phase3-evidence.md](phase3-evidence.md). Their implementation limits take precedence over proposed future behavior in the design documents.

Track delivery in [checklist.md](checklist.md). **Phase 0 research/design is complete under scope revision D14**; [phase0-evidence-review.md](phase0-evidence-review.md) records source evidence, closure and deferred native/user-validation gates. Available results are in [phase0.md](phase0.md); the later session/fixture protocol is in [phase0-validation.md](phase0-validation.md).

Phase 4 packaging work is in progress: [phase4-evidence.md](phase4-evidence.md) records qualified local results and failed/unrun native checks; [release.md](release.md) describes signed development packages, offline verification/install, upgrade and uninstall.

The follow-up [workflow research](workflow-research.md) records founder pain, scoped local tool-storage observations, native Windows volume samples and Linux/macOS/Windows source synthesis. The [read-only synthetic prototype](phase0-prototype.md) is ready for the deferred comprehension/usefulness review at the later usability gate.

| Read | Document | Purpose |
| --- | --- | --- |
| 1 | [fullcontext.md](fullcontext.md) | Complete supplied product brief, with Markdown formatting; source requirements |
| 2 | [research.md](research.md) | Internet findings, current competition, maintenance/license observations, and citations |
| 3 | [plans.md](plans.md) | Recommended implementation strategy, scope, decisions, and first work packages |
| 4 | [stack.md](stack.md) | Technology choices, alternatives, dependency admission, and build approach |
| 5 | [architecture.md](architecture.md) | Module contracts, accounting, scan/index lifecycle, and process coordination |
| 6 | [safety.md](safety.md) | Mandatory invariants, threat model, revalidation, and failure semantics |
| 7 | [rules.md](rules.md) | Catalog schema proposal, evidence, initial coverage, and authenticated updates |
| 8 | [ux.md](ux.md) | CLI/TUI behavior, selection, confirmation, accessibility, and reporting |
| 9 | [phase.md](phase.md) | Phases 0–6, deliverables, dependencies, and exit gates |
| 10 | [testing.md](testing.md) | Adversarial fixtures, platform certification, benchmarks, and release evidence |
| 11 | [operations.md](operations.md) | Packaging/authenticity, local files, recovery, services, and release operations |

Read `fullcontext.md`, `plans.md`, `stack.md`, and `phase.md` first for orientation. The remaining files supply the engineering detail needed to implement those plans.

## Requirements versus design decisions

- **Required:** Product commitments from `fullcontext.md`, especially no unattended cleanup, local inventories, independent protection, metadata-only scanning, and explicit confirmation.
- **Recommended:** The implementation decisions in the other documents. Change them with a written rationale and updated acceptance criteria.
- **Spike / unresolved:** An approach must be proven with code or real OS tests before adoption. A promising crate or API is not evidence that a safety invariant already holds.
- **Deferred:** A feature remains on the roadmap but is outside the first release scope.

When documents disagree, preserve the stronger safety requirement and record the conflict before implementation. Proposed command extensions and TOML examples are design contracts, not commands users can execute today.

## First release in one paragraph

Deliver a bounded, interruption-safe inventory of explicitly configured local user directories; fast cached startup; a small developer-focused recommendation catalog; a TUI that explains scope and uncertainty; and supported Trash operations only after their platform safety gates pass. Start with analysis-only milestones. Keep repository contents and system-managed storage informational in the initial release. Add the optional background service after foreground scanning and cleanup are reliable.

## Important interpretations

1. **Trash normally retains disk blocks.** Report “moved to Trash” independently of available-space change; never imply that trashing 10 GiB immediately frees 10 GiB. OMG does not automatically empty Trash.
2. **A directory timestamp is not a subtree checksum.** Modifying an existing descendant file may leave ancestor directory timestamps unchanged. Restart without trusted event continuity requires reconciliation.
3. **Scan permission is not cleanup permission.** A visible path may be protected; a user-supplied path or rule cannot override policy.
4. **Unknown content blocks broad removal.** A metadata-only scanner cannot identify every renamed secret or database. Narrow positive scope and protection are necessary; “we detect all secrets” is not a valid claim.
5. **Cross-platform build is not cross-platform certification.** Certify analysis and each cleanup adapter separately on real environments.

## Glossary

| Term | Meaning |
| --- | --- |
| Apparent bytes | Logical file length, including holes in sparse files |
| Allocated bytes | Filesystem-reported allocation; not necessarily unique/exclusive physical blocks |
| Observed usage | Volume capacity/available measurements and scoped indexed allocation, shown separately |
| Candidate | Rule-backed finding with evidence, explanation, tier, and freshness |
| Plan | Explicit, immutable set of targets and actions prepared for user review |
| Authorization | Session-local user confirmation bound to that exact plan |
| Coverage | Configured roots, completed/partial roots, skipped boundaries, and errors; not an invented whole-disk percentage |
| Reconciliation | Metadata enumeration that repairs missing/stale inventory independently of notifications |
| Identity | Platform file identifier scoped to volume/mount incarnation; never just a pathname |
| Protected | Core policy denies cleanup regardless of catalog or selection |

## Updating these documents

Keep the supplied brief intact. Record changes to implementation scope in `plans.md` and the affected detailed document. Refresh dated research before adopting external code. Track phase evidence in `phase.md`; do not mark a gate complete based only on a design document. Use source links for external claims and label untested behavior explicitly.
