# Recommended implementation plan

Status: proposed engineering baseline, 2026-10-13. Requirements: [fullcontext.md](fullcontext.md). Evidence: [research.md](research.md). Delivery gates: [phase.md](phase.md).

## 1. Strategy

Build a **modular Rust application with one public `omg` binary**, a SQLite-backed inventory, a declarative catalog, and a small npm launcher. Prove analysis first; add cleanup only behind a tested authorization boundary. Build on maintained low-level libraries while owning storage semantics, coverage, policy, and audit behavior.

Prioritize these outcomes:

1. Accurate scoped inventory and honest limitations.
2. Fast repeat use with the last valid snapshot.
3. Useful developer-specific explanations supported by evidence.
4. A cleanup workflow whose target set cannot silently change after approval.
5. Certified installation and behavior on each supported OS.

The first useful milestone is an analysis-only CLI and minimal read-only TUI. It should answer where storage is going even if no cleanup adapter has passed certification.

### Audience and platform priorities

The founder explicitly clarified that the product must solve storage investigation for **Linux and macOS developers broadly**, rather than specialize around their Windows/WSL machine. Prioritize native Linux and macOS discovery, explanations and validation together. Windows/WSL remain supported architectural targets with independently tested capabilities; host/VHD reporting is a platform-specific feature, not the central product pitch.

The common problem is accumulated package/tool caches, generated artifacts and application data that take time to identify and assess. Prioritize coverage using official tool semantics, independent developer feedback and native tests, not the largest directories on one machine. First research tracks are npm/pip/uv/pnpm across both platforms, Linux user caches and managed package storage, and macOS developer-tool caches/build output alongside APFS/System Data limitations. System-managed data, runtimes, repositories and backups remain protected/informational until a specific reviewed action qualifies.

## 2. Initial scope

### Analysis MVP

- Current user's configured local roots, initialized to their home directory after showing the scope on first launch.
- Exclude cloud-provider roots when safe metadata enumeration cannot be guaranteed; pseudo, network, unsupported, and automounted locations do not enter the ordinary walk.
- Do not cross filesystem/mount boundaries automatically. Add each additional volume/root explicitly and account for aliases.
- Metadata only, bounded traversal, allocated/apparent size, file identities, hard-link handling, visible errors, and cancellation.
- SQLite migrations, complete/partial generation state, last valid results, full reconciliation, and read-only diagnostics.
- `omg`, `omg scan`, `omg scan --full`, `omg status`, and `omg doctor`.

Volume capacity is a measurement of a supported volume; home-directory inventory is not a complete inventory of that volume. Show both without conflating them.

WSL additionally needs separate virtual-filesystem and Windows host-volume availability, with observation times and an unknown backing relationship when not established. Never sum their capacities or infer host headroom from virtual `df` alone. Follow-up [workflow research](workflow-research.md) found this mismatch locally and prioritizes tool-storage attribution; uv/pnpm caches, installed remote editor servers, and AI session data need distinct evidence/policy treatment.

### Recommendation MVP

- Small, independently authored npm/pip cache inventory catalog and carefully evidenced downloaded package/installer findings.
- Large files are Review only if explicitly actionable scope passes policy; otherwise explain them without an action.
- Build directories inside repositories, runtimes, SDKs, VM/container storage, device backups, OS leftovers, screenshots, and ambiguous logs begin informational or deferred until dedicated evidence/policy work passes.
- No rule based only on “cache”, “temp”, “build”, age, or size earns Safe.
- All candidates unselected initially. Safe preselection is permitted by the brief but intentionally not enabled in the first release.

### Cleanup MVP

- Ordinary targets moved to native Trash only after platform-specific race/protection tests pass.
- Explicit selected targets, immutable preview, separate confirmation, postconfirmation revalidation, sequential outcomes, and durable local audit.
- Dry-run requires explicit selection and cannot mutate target files, launch cleanup utilities, or perform staging renames. Local plan/audit writes are allowed and documented.
- No system utility adapter in the first cleanup milestone. Adding one requires fixed executable/arguments, exact scope/effects, version tests, and explicit user confirmation.

### Later

Background service, growth trends, signed online catalog updates beyond offline import, optional duplicate content analysis, and Git-aware artifact analysis are later milestones. Include their interfaces and acceptance tests early without implementing speculative infrastructure.

## 3. Decision register

| ID | Decision | Rationale / revisit condition |
| --- | --- | --- |
| D01 | Rust core and public binary; JS launcher only | Native metadata/safety APIs and predictable deployment; no end-user compiler |
| D02 | SQLite through synchronous `rusqlite`, owned writer thread | Direct transactions, bounded ingestion, fewer runtime layers |
| D03 | Evaluate `dua-core`; do not default to unmaintained `jwalk` | Current maintenance finding; admission requires proof of queue/cloud semantics |
| D04 | Separate observations, candidates, authorization, execution | Cached findings and UI state must not become deletion authority |
| D05 | User-local scope, no default whole-disk traversal | Predictable permissions/resources and accurate scope reporting |
| D06 | No repository-content cleanup initially | Brief explicitly protects repository data; metadata/ignored state cannot establish disposability |
| D07 | Fail closed on uncertain identity/scope/Trash behavior | Preserve the core safety requirements; use informational findings instead |
| D08 | No synchronous full scan on TUI startup | Render cached snapshot before bounded refresh |
| D09 | Platform npm packages without postinstall downloader | Offline-friendly, no surprise binary fetch or compiler requirement |
| D10 | Independently version app/schema/catalog/policy/adapter contracts | Catalog changes do not imply new executable capabilities |
| D11 | Analysis process coordination before background service | Same scan ownership contract later prevents duplicate service work |
| D12 | Mandatory metadata-only reconciliation after untrusted gaps | Directory mtimes/events cannot establish recursive freshness |
| D13 | Ship no Safe rules until evidence/tests qualify them | Narrow reliable recommendations are better than a reassuring unsupported label |
| D15 | Owner-approved user cleanup: an "OMG Trash" list that only saves paths, and explicit permanent deletion from it (Linux, inside the user's home only) | Requested by the owner 2026-10-14. Hard protections always deny; Git-repository membership is a confirmed warning; targets are revalidated (identity + birth time) and removed through directory descriptors. Supersedes "Trash only" (S07) for this explicit flow; see phase3-evidence.md |
| D14 | Close Phase 0 as source-backed research/design; defer unavailable native/user sessions | Owner explicitly requested this scope revision; V01–V06 in `phase0-evidence-review.md` retain later usability, safety and release gates |
| D16 | Permanent deletion (D15) allowed on ext-family, tmpfs and Btrfs as `development-unqualified`; unknown filesystems refused; no bypass flag | Owner decision 2026-10-15 (home is Btrfs). Enforced by `capability::permanent_delete` in the backend. Btrfs freed space may be less than shown (shared extents). No profile is qualified until the Linux release plan's WP05/WP12 evidence exists |

## 4. Critical-path work packages

Each package should become a small issue with fixtures, an owner, and evidence. Dependency order matters more than calendar estimates.

### WP01 — Repository/build baseline

Create a Rust workspace (`omg-core`, `omg-platform`, `omg-cli` initially), pinned stable toolchain, explicit Rust minimum version, formatting/lint CI on Windows/macOS/Linux, and dependency inventory. Choose the project license with the owner before publication. Set public executable metadata to `omg`.

### WP02 — Platform metadata spike

Implement read-only root validation and file metadata extraction on all three OS families. Test raw filenames, hard links, sparse files, mount identity, permissions, redirection, cloud exclusions, and volume measurements. Record capability support before choosing the walker.

### WP03 — Walker admission benchmark

Compare `dua-core` with a simple `walkdir` correctness reference and an OMG-controlled bounded traversal if needed. Measure internal queue peaks, huge flat directories, descriptor counts, cancellation under backpressure, boundary pruning, and no-content/nohydration behavior. A bounded output channel alone does not prove upstream queues are bounded.

### WP04 — Index lifecycle

Implement schema migrations, scan IDs, per-root coverage, bounded write batches, active/staging generations, interrupted recovery, safe pruning, corruption quarantine, and singleton scan ownership. Test power/process loss between batch commits and generation publication.

### WP05 — Analysis vertical slice

`omg scan` writes inventory; `omg status` reports scope/freshness/limits; `omg` renders a read-only overview and refreshes asynchronously. Compare accounting against a metadata oracle. Establish reproducible cold/warm/refresh benchmark data.

### WP06 — Rules and policy

Strict TOML parsing, builtin discovery/action identifiers, version checks, evidence fingerprints, protection decisions, scoped ignores, no overlapping candidate totals, and fixture-based rules. A matched rule may produce an informational/protected finding rather than an actionable candidate.

### WP07 — Cleanup safety spike

Before building a polished delete UI, prove target acquisition and Trash behavior for each supported platform. Resolve the path-based Trash race, protected-descendant checks, directory concurrency, audit crash windows, and unavailable Trash. If unresolved, keep affected targets read-only. See [safety.md](safety.md).

### WP08 — Interactive plan flow

Navigation, selection, detail/search/ignore/history, full preview, confirmation cancel default, plan fingerprint binding, dry-run, revalidation, and outcomes. The TUI receives a policy-filtered view and cannot directly call filesystem mutation.

### WP09 — Distribution certification

Build versioned signed binaries, exact-version platform packages, wrapper, native archives, provenance, clean-machine installs/upgrades, offline paths, `--ignore-scripts`, omitted optional dependencies, and uninstall documentation.

### WP10 — Optional service

Native per-user registration, scan ownership/priority, backoff, heartbeat/capabilities, threshold hysteresis, and clean stop/uninstall. Service protocol exposes scan/status operations only; no cleanup RPC exists.

## 5. Resolving specification tensions

- **Protected repositories versus build artifacts:** initial policy denies actions inside known repositories and `.git` variants. Later artifact support requires a reviewed policy extension identifying generated targets without deleting repository roots or assuming ignored data is disposable. Until then, show inventory and limitations.
- **Metadata-only versus evidence:** ordinary scans never open contents. Optional manifest reads/tool execution require a separately labelled, user-initiated analysis mode with bounded reads and no execution of project configuration. Do not silently add content reads for rule convenience.
- **Trash versus reclaiming space:** ordinary Trash processing is recoverability-oriented; physical reclamation may require the user to manage Trash independently. MVP success measures visibility and safe processing, not guaranteed reclaimed bytes.
- **Info-only versus official cleanup:** an info-only finding has no action. If a dedicated official adapter later qualifies, it produces a separately actionable Safe/Review finding with explicit consequences; catalog text cannot enable deletion on an info-only record.
- **Audit versus crash atomicity:** SQLite and filesystem moves cannot commit atomically together. Persist intent, execute, persist outcome; interrupted attempts remain unresolved until independently reconciled. Never automatically retry a destructive action.
- **Cleanup history listed as future:** history and per-target outcomes are required for Phase 3; richer restore/history features are later enhancements.

## 6. Risk and evidence register

| Risk | Response | Evidence required |
| --- | --- | --- |
| Path replacement between check and Trash | Native acquisition/claim spike, fail closed | Barrier-controlled adversarial races on every adapter |
| Unknown protected contents within broad cache directory | Positive narrow scopes; reject broad parent operations | Nested secret/database/repository fixtures |
| Cloud hydration caused by enumeration | Native attributes/provider exclusions before descent | Real online-only files with transfer observation |
| Incorrect “unchanged subtree” shortcut | Trusted invalidations plus periodic and gap reconciliation | Descendant rewrite with unchanged parent mtime |
| SQLite corruption/interrupted publication | Patched SQLite, staging generations, recovery | Fault injection, integrity checks, preserved preferences/history |
| RAM growth on giant trees | Bounded internal work; paged UI; disk-backed dedup | Queue/RSS/FD measurements on stress trees |
| Release tampering or wrong binary | Signed manifest, digest, version/target matching | Signature substitution and wrong-target install tests |
| Three OSes stretching capacity | Separate analysis/cleanup/service certification | Capability matrix and native integration results |

## 7. What to do next

Start Phase 0's unresolved platform safety spikes and WP01–WP05. Complete a measured, read-only vertical slice before enabling catalog actions. Update the relevant design documents with evidence, not guessed performance numbers. Use phase exit gates to decide readiness; do not implement all categories simultaneously or promote an untested platform merely because it compiles.
