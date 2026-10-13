# Phase 0 validation protocol and fixture design

Status: deferred validation protocol under owner-approved D14; no sessions or performance benchmarks have been completed. Phase 0 research/design is complete. This protocol remains required at V01/V05 later usability/performance gates in [phase0-evidence-review.md](phase0-evidence-review.md). Current available experiment evidence: [phase0.md](phase0.md).

## Developer prototype comparison

Recruit developers who regularly review storage, including package-heavy and multi-project workflows. **Native Linux and native macOS participants are required validation tracks**, reflecting the founder's clarified audience. Windows/WSL feedback supplements those tracks rather than replacing them. Founder preference is useful but not a substitute for independent task observation. Participant counts and session times are study-design choices, not a claim of statistical product-market proof.

Use separate synthetic scenarios:

- Linux: user package caches, generated project artifacts, mounted-volume boundaries and protected package/container-managed storage.
- macOS: developer package caches/build output, installed SDK/runtime and backup inventory, APFS/container accounting, System Data and cloud exclusions.
- Windows/WSL: remote-tool installations and distinct host/virtual capacity.

Keep the common review questions and safety model consistent. Do not show WSL-specific capacity concepts as the primary Linux/macOS workflow. Scenario paths and category names do not establish cleanup eligibility.

Use synthetic inventories and a labelled read-only prototype; do not enable cleanup or ask participants to expose sensitive filenames. Compare the same scope with current `dua`/`dust` or representative outputs and a catalog-cleaner preview. Record tool versions and disclose mocked/cached data; do not represent a static sketch as a working benchmark.

Tasks:

1. Identify where storage is going and distinguish volume usage from covered inventory.
2. Identify potentially reclaimable versus informational/protected storage and shared allocation uncertainty.
3. Explain why an installer/cache is a candidate and the redownload/offline/regeneration consequences.
4. Inspect exactly what an explicit plan would change and what Trash means for physical space.
5. Reopen a stale/partial inventory, find scan limitations, and choose whether to refresh.
6. Explain whether the prototype would improve repeated use alongside existing tools, and why.

Record:

```text
Session ID/date and explicit participation consent:
Developer workflow (nonsensitive aggregate):
OS/tool versions and synthetic fixture:
Prototype/build identity and what was mocked:
Task answers; observed confusion/errors:
Did they correctly distinguish estimates, Trash processing and space freed?
Comparison strengths/weaknesses and repeat-use preference:
Direct feedback quotes with permission, otherwise paraphrase:
Hypothesis supported, contradicted, or inconclusive:
Changes and follow-up:
```

Acceptance review: users can accurately answer the four product success questions, understand partial/freshness/Trash uncertainty, and articulate meaningful repeat-use advantage. Record contrary/inconclusive results. A checklist tick requires actual sessions and review, not the existence of this protocol. No analytics/upload collection mechanism is introduced.

## Initial adversarial fixture design

| Fixture | Purpose | Phase 0 evidence / later gate |
| --- | --- | --- |
| Two paths linked to one inode | Avoid shared identity double counting | Acquisition observed; global accounting Phase 1 |
| Sparse 8 MiB fixture | Logical versus allocation | Observed; varied native filesystems later |
| Existing child rewritten | Demonstrate ancestor mtime trust failure | Observed; reconciliation Phase 1 |
| Final/ancestor/substituted symlink | Reject redirected metadata acquisition | Observed Linux; native Windows/macOS later |
| Missing/replaced/invalid-byte name | No path retargeting; lossless names | Observed Linux; full raw path contract later |
| Cache with `.git` descendant | Broad parent cleanup must not bypass protection | Metadata observed only; recursive policy Phase 2/3 |
| Credential/database/VM/container descendant | Core protections independent of catalog | Planned; production policy adversarial suite |
| Virtual cloud directory/placeholder | Metadata enumeration can hydrate | Planned real provider observation; exclude until proven |
| Mount swap/bind alias | Identity/boundary continuity | Planned native controlled mount environment |
| Trash unavailable/partial/crash after move | No permanent fallback or invented success | Planned native handler/fault injection; no action yet |
| Tampered rule/release/plan | Verification and authorization boundaries | Planned parser/signature/plan fixtures |

## Benchmark fixture specification

Phase 0 defines fixtures; Phase 1 measures them. No “instant”, entries/sec, memory ceiling or universal timing has been demonstrated yet.

Create reproducible bounded generators with an explicit test directory and manifest recording seed/shape, intended sizes/link groups and generation parameters. Never generate inside user home caches or run destructive cleanup on real data.

| Workload | Shape | Required measures |
| --- | --- | --- |
| Many small files | Multiple directories, configurable counts and depth | Metadata calls, throughput, RSS/queue/FD peaks |
| Huge flat directory | Configurable single-directory entry count | Internal walker batching/queue bounds, cancellation |
| Deep project-like tree | Branch/depth controls, known protected metadata | Traversal/pruning, correct coverage/rollups |
| Sparse/shared identities | Holes and hard links across roots | Apparent/allocation estimates and identity union |
| Churn/permissions/renames | Controlled concurrent writers and inaccessible leaves | Partial reporting, convergence/backpressure |
| Warm/restart/incremental | Same generated tree with declared modifications | First cached frame, dirty work, gap reconciliation |

Capture OS/kernel/CPU/RAM/filesystem/disk, build/catalog/tool versions, cache conditions, configured resource limits, repetitions and variance. Empty index and cold OS caches are distinct. Test on real NTFS/APFS/ext4 targets before certification; WSL fixture results are not a substitute. Compare identical scope/accounting options with competitors. Benchmark instrumentation must not upload paths or contents.

## Completion decision

Use `checklist.md` and `phase0.md` to record actual evidence. Developer validation remains deferred/not passed until performed. D14 explicitly revises Phase 0's additional interview scheduling gate and records evidence-based closure; it does not revise mandatory product safety or release gates. Capability denials preserve safety while native certification is pending and grant no support claims.
