# Phase 0 desk-research evidence review and closure

Date: **2026-10-13**. Scope decision supplied by the project owner: temporarily skip unavailable native Linux/macOS sessions, perform deeper internet research, and complete Phase 0 on that basis.

## Acceptance decision D14

**Phase 0 is complete as a research and safety-design milestone under the revised owner-approved scope.** Independent developer prototype sessions and additional native experiments are deferred, not passed. Desk research is sufficient to choose an initial implementation direction; it is not evidence of majority preference, product-market fit, performance targets, cleanup safety or platform certification.

The supplied product brief remains preserved in `fullcontext.md`. This decision revises the additional Phase 0 interview gate introduced in the engineering plan. Mandatory product safety and release acceptance criteria remain binding.

Closure requires: current competitor/dependency research; a clear differentiator and explicitly provisional audience assumptions; sourced Linux/macOS storage mechanisms; mandatory policy/threat model; tested available-environment feasibility or capability denial; explicit scope/action consequences; benchmark/adversarial design; and an owned deferred-validation register. Those artifacts now exist in the documentation and Linux spike.

## Evidence method

Prefer official tool/platform documentation for scope and consequences; use issue reports for concrete failure cases; record direct founder feedback separately. Existing research comprises `research.md` (32 source references), `workflow-research.md` (12 official workflow/tool references, some overlapping earlier research), and the additional retrieved sources below. Counts are references, not unique sources or independent user samples.

No representative survey was found or conducted in this work. Documentation and issue reports are purposive evidence of supported behavior and failure modes, not estimates of prevalence. Exact dependency versions, source licenses and native behavior must be rechecked when adopted. No cleaner/database code was copied from competing projects.

## Additional source-to-decision matrix

All sources accessed on 2026-10-13; command descriptions are research only and were not executed.

| ID / source | Retrieved evidence | OMG decision / limit |
| --- | --- | --- |
| E01 — [Ubuntu Noble apt-get manual, APT 2.8.3](https://manpages.ubuntu.com/manpages/noble/en/man8/apt-get.8.html) | `clean` removes retrieved archives including partial location; `autoclean` and `autoremove` have different scopes. Simulations disable locking and may miss unreadable configuration. | Separate cache from installed packages; managed/partial data remains protected. A preview is not final execution proof. No blanket elevated APT adapter in MVP. |
| E02 — [systemd journalctl manual mirror](https://man7.org/linux/man-pages/man1/journalctl.1.html) | `--disk-usage` includes active and archived journals; vacuum acts on archived journals, so usage and removable bytes differ. | Journals informational; never read log contents in ordinary scan. A future fixed official adapter needs precise scope/privilege and irreversible-history warnings. |
| E03 — [Homebrew FAQ](https://docs.brew.sh/FAQ) | Cache discovery via `brew --cache`; old formula versions and periodic cleanup are managed by Homebrew and configurable. Architecture-dependent prefixes can coexist. | Resolve actual ownership/layout; do not infer abandoned versions from age or duplicate prefixes. Cache and installed runtime inventory are different. |
| E04 — [Homebrew manual, cleanup section](https://docs.brew.sh/Manpage#cleanup-options-formulacask-) | Cleanup affects stale locks/outdated downloads and old installed versions; supports dry-run; scrub/prune alter effects. | “Official cleanup” may exceed cache scope. No generic shell command or unreviewed whole-prefix removal; inspect exact version/environment before any adapter. |
| E05 — [Apple Time Machine local snapshots](https://support.apple.com/en-us/102154) | Snapshots enable restoration; Apple counts snapshot space as available and manages aging/space pressure. | Snapshots info-only. Distinguish measured filesystem free bytes from OS notions of available/purgeable space. No promise that snapshot size is additional reclaimable space. |
| E06 — [Apple Xcode component management](https://developer.apple.com/documentation/xcode/downloading-and-installing-additional-xcode-components.md) | Components settings shows installed components/storage recovery and supports explicit removal. Simulator/platform downloads and imports are tool-managed. | Inventory SDK/simulator components with official guidance; no direct managed-directory deletion or assumption “unused.” Native adapter/version tests required later. |
| E07 — [uv storage reference](https://docs.astral.sh/uv/reference/storage/) | Distinct dependency cache, persistent installed Python/tool directories, configuration and project/script environments; overrides and platform-specific paths. | Cache names do not authorize deleting installed tools. Pair this with uv's documented direct-modification prohibition and locks from workflow research. |
| E08 — [Cargo clean reference](https://doc.rust-lang.org/cargo/commands/cargo-clean.html) | Default clean removes the entire target directory; target-dir may be configured; dry-run/verbose previews scope. | A `target` name alone is insufficient evidence. Repository protection remains mandatory; tool config/command execution requires reviewed boundaries. |
| E09 — [dua issue #410](https://github.com/Byron/dua-cli/issues/410) | Reproducible Windows long-root failure caused by changing process working directory; fixed upstream. | Keep raw native paths and explicit roots; do not rely on process `chdir` for traversal. Add native long-path regression tests. |
| E10 — [dua issue #408](https://github.com/Byron/dua-cli/issues/408) | Reported crash with lone-surrogate Windows names during pattern filtering; fixed upstream. | Lossless filenames and exclusions are correctness/safety requirements. Issue proposal is not the final implementation authority; validate exclusions on raw native representation. |

The direct systemd journalctl documentation returned HTTP 418; E02 is the successfully retrieved manual mirror. The original Apple simulator URL returned a JavaScript landing page; E06 is the linked Markdown content retrieved successfully. These limitations do not count as native execution evidence.

## Cross-platform evidence synthesis

### Linux

Evidence supports separate classes: user package caches (npm/pip/uv/pnpm), generated artifacts, installed runtimes, managed package archives, container state, and active/archived journals. They differ in regeneration, shared links, privilege, ongoing writes and user value. Start with user-scoped metadata inventory and narrow rule explanations; system/package/container-managed storage remains informational. Existing `dua`/`dust`/GNOME tools already cover usage visibility, so OMG's hypothesis is recurring, coverage-aware intelligence and evidence-backed review.

### macOS

Evidence supports developer-tool downloads/caches and installed Xcode components alongside broader app data, backups and system-managed/purgeable/shared storage. Homebrew ownership/prefixes and Xcode component management need structured version-aware treatment. Apple Storage/System Data and Time Machine evidence show why category sizes, APFS allocation, snapshots and available space cannot be treated as additive removable bytes. No general “clear Library caches” rule qualifies.

### Windows/WSL

Existing local read-only volume evidence demonstrates a real host/virtual-capacity mismatch; upstream failures demonstrate long-path and lossless-name risks. They are additional platform requirements, not the core Linux/macOS pitch. Windows cloud flags, junctions, handles and Recycle Bin outcomes still require native tests before enabling the related features.

## Research-backed implementation priorities

1. Shared scoped metadata accounting, cached startup, transparent coverage and reconciliation.
2. Independently authored npm/pip/uv/pnpm inventory/explanations on Linux/macOS, with root overrides, shared bytes and offline consequences.
3. Explain generated build output and installed tools separately; keep repository/runtime removal disabled until reviewed policy evidence exists.
4. Informational platform adapters for APT/journals, Homebrew/Xcode/snapshots and Windows/WSL volume distinctions.
5. Exact preview/confirmation, race-safe acquisition, Trash-only ordinary operations and truthful outcomes after native certification.

Do not promise a minimum reclaimed size, majority preference, universal speed or automatic performance improvement. “Regenerable” can still imply network/time/offline loss and does not override protection.

## Deferred validation register

Project maintainer owns these follow-ups; no participant or native result is assumed.

| ID | Deferred work | Due gate | Acceptance |
| --- | --- | --- | --- |
| V01 | Independent native Linux/macOS developer prototype comparison | Phase 3 usability acceptance; before claiming validated usefulness | Record task answers, confusion, competitor comparison and contrary findings; revise design if needed |
| V02 | Native Linux filesystem and macOS metadata/allocation/cloud tests | Phase 1 analysis capability certification; Phase 4 supported release | Correctness, no hydration, coverage and limits pass on each advertised environment |
| V03 | Native Windows reparse/raw path/identity tests | Corresponding Phase 1/4 capability certification | No redirected traversal, lossy names or unsupported metadata claims |
| V04 | Trash/Recycle Bin target races, protected descendants, partial failure and recovery | Phase 3 action enablement; Phase 4 release | Mandatory safety suite passes; absent evidence means action disabled |
| V05 | Representative cold/warm/incremental resource benchmarks | Phase 1 measurement gate; Phase 4 regression review | Record hardware/tree/cache/queue/RSS/FD methodology; set targets from measurements |
| V06 | Real catalog/tool layout/version/command tests | Phase 2 rule activation and Phase 3 adapter enablement | Fixtures, official semantics, core policy and exact effects verified |

CI hosted runners may help with native builds/tests when configured, but compilation alone does not satisfy cloud/provider/Trash behavior or actual user feedback. No native machine purchase/access is required to close this design milestone; unsupported capabilities stay denied until evidence exists.

## Final Phase 0 review

- Product brief, broad Linux/macOS focus and explicit non-goals preserved.
- Competitive differentiator is clear and provisional, supported by sourced mechanisms and founder pain rather than claimed majority validation.
- S01–S12, threat model, authoritative policy, protected roots and immutable authorization design documented.
- Available Linux synthetic experiment passed 11 checks and six tests; other native capabilities explicitly denied.
- Catalog/dependency/license/provenance, adversarial fixtures and benchmark plans documented.
- Owner-approved deferral is recorded with concrete later gates, including user feedback before validated-usefulness claims.

**Phase 0 closure: accepted under D14. Phase 1 is next. No production or platform-release gate is marked passed by this decision.**
