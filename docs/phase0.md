# Phase 0 evidence and completion assessment

Assessment date: **2026-10-13**. Checklist: [checklist.md](checklist.md). Requirement gates: [phase.md](phase.md). Reproduction: [probe README](../spikes/phase0/README.md).

## Conclusion

**The local Phase 0 engineering/design deliverables are verified. Full Phase 0 is not 100% complete:** developer prototype comparison has not occurred. Founder feedback supports a direction but does not prove the four success questions or repeat-use value. Native cloud/Trash/Windows/macOS capabilities are explicitly denied pending future certification; this satisfies the Phase 0 requirement to avoid advertising unproven capabilities, not certification itself.

No cleanup capability is implemented or enabled. Rust production work starts in Phase 1. This environment has no Rust toolchain; the dependency-free Python spike tests Linux kernel feasibility without selecting a Rust dependency or weakening any production requirement.

## Gate-by-gate review

| Phase 0 gate | Evidence | Assessment |
| --- | --- | --- |
| Current competitors/component licensing researched | `research.md`, 32 source references and dated maintenance findings | Design research complete; refresh exact versions at dependency admission |
| Defensible differentiator defined | `plans.md`, founder feedback below | Hypothesis established, independently unvalidated |
| Actual prototype feedback validates four questions/repeat use | Session protocol in `phase0-validation.md` | **Pending; Phase 0 exit blocker** |
| Default scope/exclusions/repository protection explicit | `plans.md`, `safety.md`, `architecture.md`, `ux.md` | Design complete |
| Mandatory safety invariants and adversarial plan | S01–S12 in `safety.md`, fixture expectations in `testing.md` | Design complete; production safety tests not yet built |
| Native feasibility results or capability denials | Executable Linux spike, matrix below, Trash acquisition plan in `safety.md` | Phase 0 evidence complete within declared limited scope |
| Dependency/license/provenance and benchmark strategy | `stack.md`, `operations.md`, fixture design in `phase0-validation.md` | Design complete; license adoption deferred to owner/Phase 1 |

## Actual founder feedback

- Source: direct response to the Phase 0 validation question in this development session.
- Question: “Based on your own workflow, which repeat-use benefit would make OMG worth using alongside existing disk tools?”
- Offered options, in order: explanations/safety; fast cached overview; developer-tool coverage; still needs validation.
- Exact response: **`1, 2, 2`**.
- Interpretation: prioritizes explanations/safety and cached startup; repeated `2` is recorded rather than invented as an additional preference.
- Limitation: one founder preference response; no prototype was shown, no competitor task was observed, no representative developer interview/sample or performance validation occurred.

The hypothesis to test is: **trustworthy explanations and exact scope, presented immediately from a local coverage-aware index, make repeated storage review materially more useful.** See the validation protocol for required evidence rather than treating this preference as product-market validation.

## Executable experiment results

Commands executed successfully from repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 spikes/phase0/metadata_probe.py
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s spikes/phase0 -p 'test_*.py' -v
```

Environment: Python **3.12.3**, Linux **x86_64**, kernel **5.15.167.4-microsoft-standard-WSL2**. Synthetic fixtures were created on the temporary filesystem available to this environment; no specific physical disk, ext4 deployment or Windows host filesystem certification is inferred.

Result: **11/11 probe checks passed; 6/6 unittest cases passed**, with no skips in this environment.

| Observation | Confirmed scope |
| --- | --- |
| Shared identity/link count | Two fixture hard links expose the same device/inode and at least two links |
| Sparse size/allocation | Fixture logical length exceeds reported allocated bytes (`st_blocks × 512`) |
| Metadata-only acquisition | Linux `O_PATH` descriptor cannot return content via `read` (`EBADF`) |
| Final/ancestor/substituted symlinks | `openat2` rejects fixture redirection with no fallback |
| Parent/root escape | Anchored acquisition rejects `../` escape; absolute/NUL/empty inputs rejected |
| Replacement identities | Replacement path resolves to a different inode; retained handle still identifies original |
| Parent timestamp counterexample | Rewriting an existing fixture descendant preserves the parent mtime |
| Protected-descendant metadata | `.git` directory fixture is visible as metadata; no policy classification is claimed |
| Native filename/missing target | Non-UTF-8 fixture roundtrips; missing path returns an error without retargeting |
| Unsupported platform | Mocked Windows platform denies acquisition with no weaker fallback |

These tests do not traverse arbitrary user roots, read file contents for analysis, mutate user files, execute Trash, prove recursive protections, or establish bounded production scanning. Synthetic fixture setup/teardown is explicitly separate from application behavior.

## Capability matrix

| Environment/capability | Phase 0 status | Follow-up |
| --- | --- | --- |
| WSL2 Linux x86_64 metadata acquisition/no-follow | Experiment verified on fixture scope | Implement/test Rust adapter and bounded traversal in Phase 1 |
| Linux aarch64 syscall profile | Probe code restricted to known syscall number; not executed | Native hardware test before support |
| Linux ext4/other real storage accounting | Not certified by WSL fixture results | Native representative machine/filesystem tests |
| Live mount swaps/bind aliases | No-cross-device flag used; live substitution not tested | Native mount/boundary adversarial suite; deny affected action |
| Windows NTFS/reparse/cloud | **Denied/unverified** | Native metadata/identity/flags probe; cloud provider integration |
| macOS APFS/File Provider | **Denied/unverified** | Native nonhydrating metadata and pool/identity tests |
| Cloud nonhydration, any provider | **Denied/unverified** | Exclude unsafe roots; observe real provider transfer/hydration behavior |
| Trash/Recycle Bin execution, all platforms | **Denied/unimplemented** | Prove stable target semantics/descendants/outcomes before Phase 3 activation |
| Service, all platforms | **Denied/unimplemented** | Per-user native integration/scan-only coordination in Phase 5 |
| Ordinary production scanning | **Unimplemented** | Phase 1 Rust engine, index, resource and interruption tests |

## Confirmed design decisions

1. Ordinary scans are metadata-only and explicitly scoped; cloud redirection and uncertain traversal are excluded.
2. Native handles aid observation; a later path-based Trash call remains a separate race-safety gate.
3. Directory mtime is insufficient to trust descendants; notifications are advisory and reconciliation mandatory.
4. Hard links, logical/allocation sizes, selected estimates and physical available-space delta stay distinct.
5. Core policy owns authorization; unknown/protected/info data never obtains cleanup authority through a rule or UI.
6. Developer repository data is protected initially; no ignored/clean/pushed shortcut to disposal.
7. Actual cleanup requires separately reviewed exact plan confirmation and immediate revalidation, durable intent/outcomes and no permanent fallback.
8. Independently authored small catalog, verified releases, local inventories and no unattended/service cleanup remain mandatory.

## Remaining Phase 0 exit work

Conduct the prototype/competitor sessions in `phase0-validation.md`, record actual outcomes and limits, and review whether the differentiator succeeds. There is no fabricated interview result or “100% confirmed” claim. If feedback contradicts the hypothesis, revise scope before expanding implementation. Native unproven capabilities remain denied until their later certification gates pass.
