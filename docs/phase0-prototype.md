# OMG — read-only Phase 0 review prototype

**Static synthetic prototype.** All sizes, paths, freshness labels, evidence and operations below are invented fixture data, not live analysis, tested rules, a working TUI, or approved deletion. No filesystem action can be executed from this document. The layout explores explanations and WSL/Windows accounting based on [workflow-research.md](workflow-research.md).

## Shared Linux/macOS overview variants

Linux and macOS are primary validation tracks. The WSL screens below are an additional scenario, not the universal product layout. Each variant uses the same details/preview/outcome pattern and begins with no selection:

| Synthetic finding | Linux scenario | macOS scenario | Initial policy |
| --- | --- | --- | --- |
| Package/tool cache | npm/pip/uv/pnpm under validated user roots | Same tools under platform-specific validated user roots | Info until a reviewed adapter qualifies |
| Generated build output | Project build output requiring repository protection | Xcode/project build output requiring precise scope and evidence | Info; no name/age-based Safe assignment |
| Installed runtimes | Compiler/SDK/runtime inventory | SDK/simulator/runtime inventory | Info; no proof of nonuse |
| System-managed storage | Package/container-managed storage | System Data, snapshots and shared APFS storage | Info; no direct deletion |
| Downloaded installer | Exact synthetic file | Exact synthetic file | Review only if future rule/policy gate passes |

The native overview displays supported volume/pool capacity, available space, scoped inventory, last successful reconciliation and coverage. macOS shared containers must not be counted as independent physical capacities; Linux mount boundaries and permissions remain visible. Prototype participants evaluate whether explanations shorten investigation and clarify consequences across these scenarios.

## Screen 1 — Where is storage going?

```text
OMG — Storage intelligence                 READ-ONLY PROTOTYPE

Windows host C:     200 GiB capacity       12 GiB available
WSL virtual root:  1000 GiB capacity      850 GiB available
Backing relationship: unknown. These capacities must not be added.

Scope: synthetic home inventory; host files NOT inventoried
Last completed metadata reconciliation: yesterday
Refresh pending. Cloud, credentials and inaccessible roots excluded.

Potentially reclaimable estimate: 0.50 GiB  Selected estimate: 0 B
Inventory below does not sum to whole-volume used storage.

Category       Finding                    Tier       Estimate
Package tools  uv-managed cache            Info        2.00 GiB
Editor tools   installed remote servers    Info        8.00 GiB
Downloads      old SDK installer           Review      0.50 GiB
Projects       project with node_modules   Protected   3.00 GiB
AI tools       local session database      Protected   0.25 GiB

[Inspect] [Preview selected] [Refresh] [Show coverage] [Quit]
All items unselected. Info/Protected items have no cleanup action.
```

## Screen 2 — Why is this a finding?

**uv-managed cache — Info only, estimated allocation 2.00 GiB**

- Fixture location: `<synthetic-home>/.cache/uv`.
- Meaning: downloaded/built Python packages and managed cache state reduce repeated downloads/builds.
- Evidence limitation: this mock asserts tool ownership; a production rule needs native scope, layout/version and activity evidence.
- Consequences: future downloads or expensive rebuilds may be required; offline work can depend on cached packages; hard links may share blocks with environments.
- Action: **none**. uv forbids direct cache modification. A future reviewed official adapter must establish exact version/scope/effects and locking first.
- “Cache” does not mean “all bytes are safe or immediately recoverable.”

**Installed remote editor servers — Info only, 8.00 GiB**

These are installed tool binaries/extensions, not necessarily disposable download cache. Multiple versions do not prove nonuse. No server or extension will be removed by this prototype.

**Local AI session database — Protected**

May contain valuable history or working state. Database protection applies even if a tool calls its parent a cache. No action is exposed.

## Screen 3 — What would an approved plan change?

This illustrative screen assumes the synthetic installer has passed a future tested rule/policy gate; no such handler exists today.

```text
PLAN mock-001 — PREVIEW ONLY
Targets: 1 exact file
Path: <synthetic-home>/Downloads/sdk-installer-old.zip
Action: Move to native Trash
Estimated allocated size: 0.50 GiB
Rule: fixture.installer.review v1 (not a real catalog rule)

Warnings:
- Keep it if needed for reinstall/offline work.
- Trashing usually retains disk blocks; immediate space freed may be 0 B.
- Revalidate exact identity/scope/protection after confirmation.
- A changed/replaced target cannot silently replace this target.
- WSL available space and host available space are different measurements.

No caches, projects, installed servers or databases are part of this plan.
[Back] [Cancel — default]
Execution unavailable in this read-only prototype.
```

## Screen 4 — Outcome language

Synthetic example only: “1 item processed into Trash; 0 skipped; 0 failed. Processed estimate: 0.50 GiB. Observed available-space change: 0 B. The item remains in Trash and concurrent storage activity can affect measurements.”

No actual operation or volume measurement accompanies this screen. A production result requires native known outcomes/receipts, audit and before/after samples.

## Review questions — answer before reading the interpretation

Please answer in your own words; “unclear” is useful feedback.

1. Which environment appears storage-constrained, and why does WSL's large available number not settle Windows capacity?
2. Which finding can be selected in the mock, and why are the larger caches/servers/projects not included?
3. What exactly would `mock-001` change? Could it remove an AI database or `node_modules`?
4. Would moving 0.50 GiB to Trash guarantee 0.50 GiB more available space?
5. Compared with your current `df` → AI → manual investigation workflow, what would save time here, and what is missing/confusing?

The review tests separation of capacity/scope, evidence/policy, exact authorization and physical outcomes. Correct answers show comprehension of this sketch, not production safety or performance. Record direct participant responses and limitations in Phase 0 evidence; do not invent a successful session or claim majority validation.
