# Verification, benchmarks, and release evidence

Status: production test plan; production tests have not been implemented or run. The limited Phase 0 feasibility probe and its six tests have passed on this WSL2 Linux environment; see [phase0.md](phase0.md) for scope and results. Safety invariants: [safety.md](safety.md). Phase gates: [phase.md](phase.md).

Phase 0 research/design closure under D14 does not satisfy native testing, usability or performance acceptance. V01–V06 in [phase0-evidence-review.md](phase0-evidence-review.md) assign deferred independent developer review, metadata/cloud certification, Trash races, benchmarks and real-tool qualification to their later gates.

## 1. Test layers

| Layer | Purpose |
| --- | --- |
| Model/policy unit tests | Denial precedence, path-component semantics, overlap/identity accounting, state transitions |
| Property tests | Arbitrary path names/topologies, no policy downgrade, monotonic plan scope, accounting union properties |
| Filesystem integration | Real metadata, permissions, links, sparse allocation, native flags and mount behavior |
| Adversarial execution | Controlled replacements/concurrency/partial Trash failure, confirm/audit boundary |
| Crash/fault injection | Interrupted DB batches/publication, disk-full, mutation-intent/outcome gaps |
| Catalog fuzz/fixtures | Strict bounded parsing, version/actions/scope signatures and negative rules |
| TUI/CLI | Keyboard flows, narrow layouts, escaped names, no destructive input ambiguity |
| Packaging/native release | Actual installation, binary authenticity, upgrades, offline and service lifecycle |
| Benchmarks | Cold scan, warm startup, refresh, peak resource use and correctness under load |

Use synthetic disposable fixture trees in isolated test profiles and controlled Trash environments. Never point destructive tests at developer home/system data. Real native Trash integration tests need isolation and explicit test consent; removing test fixtures afterward is test maintenance, not production cleanup behavior.

## 2. Adversarial safety suite

| Fixture / action | Expected outcome | Invariants |
| --- | --- | --- |
| Candidate selected without confirmation | No mutation | S01, S02 |
| App restart, Safe candidate, saved plan or service notification | No automatic action | S01, S02 |
| Forged index/finding claims system/root/home target is Safe | Denied by fresh policy | S03, S06 |
| Symlink in target or intermediate component | No follow/action | S05, S06 |
| Parent swapped to symlink after preview/confirmation | Skip/deny, no protected target processing | S05, S06 |
| Windows junction/reparse mount/unknown tag substitution | No redirected traversal/cleanup | S05, S06 |
| Same pathname replaced with new inode/file ID | No retargeting to replacement | S06 |
| Mount detached/replaced or bind alias changed | Deny stale scope; truthful coverage | S05, S06 |
| Candidate directory contains `.git` dir/file, bare repo, worktree, nested repository | Denied, including parent operation | S03 |
| Cache parent contains credential, SQLite+WAL, VM disk, container volume, partial download | Denied broad operation | S03, S06 |
| Protected descendant inserted during final enumeration/acquisition | No parent Trash until safety established | S03, S06 |
| Online-only placeholder/virtual cloud directory | No content open or enumeration fetch; excluded/limited coverage | S04 |
| Info-only/unknown finding action requested through API/CLI | Denied independently of UI | S03 |
| Trash unavailable/full/permission blocked | Failed, no unlink/remove fallback | S07 |
| Adapter partly processes target list | Accurate per-target known/unknown results | S10, S11 |
| Cancel before/after native move boundary | No new targets; current step finishes/reconciles | S01, S10 |
| Crash after intent, during claim, after move, before outcome | Unresolved state; no automatic replay/overwrite restore | S10 |
| Audit write fails before operation | Mutation never starts | S10 |
| Catalog contains shell command, root escape, unknown action, privilege escalation | Catalog rejected or action denied | S03, S08, S09 |
| Tool executable/config/PATH replaced or project config executes code | Adapter denied; ordinary scan never executes it | S09 |
| Plan changes group membership/action/warnings/catalog after confirmation | Authorization invalidated; re-review required | S01, S06 |
| Filename includes newline, ANSI/OSC, bidi, invalid bytes/UTF-16, leading dash | Safe display/path preservation; no SQL/shell escape | S03, S09, S12 |

Use barrier hooks at preview/final validation/acquisition/handler boundaries to deterministically introduce races. Supplement with stress runs, but do not rely solely on nondeterministic sleeps. Native limitations discovered here must disable the affected capability rather than add a bypass flag.

## 3. Scanner/accounting correctness suite

- Nested directory rollups: a grandchild file contributes once to global inventory; parent/child display totals are not summed together.
- Hard links across directories/roots: unique inventory correct; selection with an unselected/out-of-scope link does not claim exclusive reclaimable blocks.
- Sparse/compressed files: apparent versus allocated size differs as expected; missing allocation metadata stays unknown.
- Clone/reflink/snapshot fixtures: allocation uncertainty labelled; no unsupported exclusive-block claim.
- Overlapping roots and mount aliases: no repeated physical inventory/capacity pool double counting.
- No-follow root and ancestor cases, special files, pseudo filesystems, boundary exclusions, disconnected mounts.
- Unix raw names and Windows long paths/lone surrogates: no panic/lossy operation paths.
- Permission failure and deletion/rename during scan: root becomes partial/changing; not complete.
- Huge flat directory, deep tree, million-entry fixture: measured queues/RSS/FD bounds and cancellation.
- Existing descendant rewrite with unchanged ancestor mtime: reconciliation detects size changes; no invalid “unchanged subtree” shortcut.
- Watcher overflow, missed events, startup gap, directory/root rename: broader invalidation and reliable rescan.
- Content-open instrumentation: ordinary scanning does not read payloads; audit all code paths, including rule discovery and watcher polling.
- Real cloud providers: record network/hydration counters before/after enumeration; mocked flags alone do not certify nonhydration.

Compare output with a simple metadata oracle using the same scope, exclusions, file identities, and allocation availability. `du`/`dust` are useful cross-checks but differing symlink/link/allocated/pool semantics can produce legitimate differences; explain them before declaring an error.

## 4. SQLite and coordination reliability

Inject process termination at scan creation, every batch commit, root publication, pruning, migration, audit intent, handler completion and outcome persistence.

Require:

- Last valid snapshot survives incomplete staging; no successful timestamp advances on failure.
- Missing entries are pruned only after successful parent enumeration.
- Concurrent CLI/service contenders have a single scan owner; two cleanup sessions cannot process the same target concurrently.
- Migrations are transactional, tested from every supported old schema, backed up consistently, and reject unsupported downgrade.
- `SQLITE_BUSY`, disk-full, permission/I/O failure and genuine corruption are distinguished.
- Recovery preserves readable preferences/history and quarantines corrupt cache safely; DB/WAL are treated consistently.
- WAL checkpoint/reader starvation and DB growth are bounded under paged reads and long scans.
- Runtime SQLite satisfies maintained/patch requirements; index stored on unsupported/network state path is rejected.
- PID reuse/heartbeat loss/restart do not override an existing kernel lock.
- IPC checks peer identity/message size/protocol and exposes no cleanup action.

Filesystem move + audit is not atomically commit-able across SQLite/native APIs. Test that this limitation yields unresolved truthful records, never fabricated success.

## 5. Rule/catalog verification

Every activated rule requires real positive and negative fixtures, source citations, tested tool/platform versions, clear side effects, and effective policy results. Test unknown versions/layouts, protected child insertions, config-root redirects and overlapping candidates.

Update tests: tampered signature/payload, wrong trusted key, replay sequence, explicit signed rollback, key rotation, missing file, duplicate path, path traversal/symlink archive, oversized input, incompatible schema/engine, unknown action, interrupted activation and last-known-good retention. Fuzz parsers/normalization within resource budgets.

No Safe rule ships based only on a happy-path fixture or a category name.

## 6. TUI/CLI verification

- Snapshot/render and event-sequence tests at 60×20, 80×24, 120×40; tiny-terminal cancel/quit remains available.
- Keyboard-only navigation, focus, filters, exact ignore scope, history, full path inspection.
- Selection updates estimates immediately; refresh does not select new identities or add targets to a frozen plan.
- Preview and separate cancel-default confirmation; repeated Enter/navigation cannot accidentally authorize.
- Info/protected controls disabled and backend rejects equivalent forged requests.
- Terminal restoration after quit, interrupt, panic and resize.
- Sanitized Unicode/invalid path names and monochrome/non-color state cues.
- Nonterminal invocation does not start raw mode; dry-run requires explicit target selection and changes no target files.
- Result screen correctly distinguishes failed/skipped/unknown and zero/negative storage delta.
- File-manager integration opens eligible location with fixed argv/native API, never arbitrary file/shell execution.

Do not assume render snapshots prove native terminal or filesystem behavior; use real terminal smoke tests too.

## 7. Benchmark methodology

Performance budgets are established **after** measurement, not invented universal timings.

| Workload | Measure |
| --- | --- |
| Cold full scan | Wall time, entries/sec, metadata calls, CPU, RSS, queue/FD peaks, DB/WAL size |
| Warm startup | Process start to first useful cached frame, query latency, index reads, render responsiveness |
| Incremental dirty scopes | Work enumerated, freshness delay, bytes/entries changed, resource peaks |
| Restart-gap reconciliation | Time/work compared to cold scan, no stale completeness claims |
| Cancellation | Stop scheduling latency, worker join duration, no deadlock under queue/DB backpressure |
| Background service | Idle CPU/RSS, scan CPU/I/O, battery-aware behavior where supported, backoff |
| Cleanup preview | Revalidation cost, completeness/denial, UI responsiveness for large exact target sets |

Representative fixtures: many tiny files, single huge directory, deep nested projects, large sparse files, hard links across roots, permission-denied directories, changing trees, SSD/NVMe and rotating disk where available. Run native Windows NTFS, macOS APFS, Linux ext4 and Btrfs. Unknown/cloud/network environments are correctness/denial cases, not an opportunity to claim scan speed.

Record build/compiler/catalog versions, OS/kernel, CPU/RAM, disk/filesystem, tree entry counts/shape, bytes, cache conditions, exclusions, worker/resource settings, sample count and percentiles/variance. An empty app index is not necessarily an OS cold cache; distinguish these explicitly. Do not drop host caches or disturb user workloads without an explicitly controlled benchmark environment.

Compare with `dua`/`dust` using equivalent metadata/scope semantics. Both correctness and resource peaks matter. Establish baseline regression budgets per representative class, retain anonymized aggregate results, and investigate material regressions before release.

## 8. Platform certification and packaging

Track analysis/Trash/service independently for each OS version/filesystem/architecture. Native environments are required for certified actions; CI compilation or emulator results are supplementary.

Test npm global install, upgrade, uninstall, `omg` argv/signal/exit propagation, no Rust/Node native compiler requirement, target/libc/version resolution, missing optional dependencies, `--ignore-scripts`, offline package/manual archive, bad signature/digest, tampered binary, permission-restricted prefix, paths with spaces and native runtime dependencies.

Service tests include explicit registration, logged-in/session limitations, startup/backoff, duplicate work, stop/uninstall, binary path across upgrades, unhealthy heartbeat, missing user manager and notification hysteresis. Service never cleans automatically.

## 9. Release checklist

- [ ] S01–S12 suite passes on every enabled cleanup capability.
- [ ] No unsupported platform/category is advertised as certified.
- [ ] Scanner coverage/accounting/interruption and incremental gap tests pass.
- [ ] Every active rule has evidence, sources, version fixtures and policy decisions.
- [ ] Catalog and binary verification fail closed; provenance/SBOM/notices included.
- [ ] SQLite migrations/recovery/locks and audit crash states pass.
- [ ] TUI keyboard/confirmation/narrow layout/terminal restoration pass.
- [ ] Cold/warm/refresh/resource benchmarks recorded with regression review.
- [ ] Clean install/upgrade/offline/uninstall and service lifecycle tested where delivered.
- [ ] Threat model, security reporting channel, known limitations, release matrix and user docs published.
- [ ] No telemetry/inventory-upload paths or unattended cleanup option exists.

Before release, store build-specific evidence using the template in `phase.md`; unchecked gates are actual blockers, not documentation boilerplate.
