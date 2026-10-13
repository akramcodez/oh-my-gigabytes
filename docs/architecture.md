# Architecture and data contracts

Status: proposed. Stack: [stack.md](stack.md). Safety boundary: [safety.md](safety.md).

## 1. System shape

```text
npm launcher -> verified omg native binary
                         |
                   CLI / ratatui
                         |
                 application coordinator
                    /           \
          query/scan path       explicit plan path
              |                       |
      scanner -> index -> rules    policy -> authorized action -> audit
              ^                       |
      advisory invalidations      platform adapter
              |
       optional scan service
```

The UI queries findings and requests plans. It has no direct deletion API. The service can request scans and update observations but cannot authorize or execute cleanup. Native adapters implement primitives; core policy decides whether those primitives can be used on a particular target.

## 2. Modules and responsibilities

| Module | Inputs / outputs | Ownership |
| --- | --- | --- |
| Model | Typed observations, identities, coverage, rule evidence, plans, outcomes | No I/O; lossless native paths separated from display strings |
| Platform | Root probes, metadata, volume samples, native acquisition, Trash/file-manager/service primitives | Capability detection and OS-specific behavior, no tier promotion |
| Scanner | Validated `ScanScope`, limits, cancellation -> bounded observation/coverage events | Metadata-only traversal; no cleanup dependency |
| Index | Observation batches -> committed generations; paged query results | Migrations, consistency, pruning, preference/history persistence |
| Rules | Verified catalog + inventory/evidence -> findings | Discovery and explanations, never operation authority |
| Policy | Finding/target + current metadata + mandatory protections -> allow/deny reason | Sole authorization boundary; catalog/preferences cannot weaken it |
| Planner/executor | Explicit IDs -> frozen preview -> session authorization -> per-target results | Overlap elimination, revalidation, audit state machine |
| TUI/CLI | Keyboard/typed commands -> application requests | Presentation, explicit confirmation, redacted errors |
| Service | Schedule/invalidation -> scan requests | Low-priority maintenance, status, no cleanup method |

Keep blocking work away from terminal rendering. Use bounded event queues with coalesced progress; critical errors/outcomes cannot be dropped. Query pages rather than loading a million-entry tree into RAM.

## 3. Core types

Proposed contracts, not final Rust signatures:

```text
NativePath { platform_encoding, raw_units, display_escaped }
VolumeIdentity { stable_id_if_available, mount_incarnation, storage_pool_id? }
FileIdentity { volume_identity, native_file_id, birth_or_generation_hint? }
Observation { parent_id, name_raw, type, identity?, apparent_bytes,
              allocated_bytes?, links?, timestamps?, native_flags,
              observed_at, scan_generation, confidence, limitations }
Coverage { scope_id, root_states, excluded_boundaries, errors, reconciled_at? }
Finding { rule_id, rule_version, catalog_digest, targets, evidence,
          requested_tier, effective_policy, explanation, warnings, freshness }
CleanupPlan { id, fingerprint, catalog_digest, policy_version, items,
              estimated_bytes, warnings, created_at, expires_at }
Authorization { plan_fingerprint, session_id, confirmed_at, one_use }
Outcome { attempted_action, status, error_code?, receipt?, before_estimate,
          after_volume_sample?, reconciliation_state }
```

Represent absence of allocated size or identity as unknown, not zero. Keep native inode/file IDs in lossless bytes rather than squeezing unsigned platform IDs into SQLite signed integers. File IDs can be reused; add observation metadata and mount incarnation, then revalidate at execution.

Unix names use original bytes; Windows names use lossless UTF-16 units. A sanitized UTF-8 display/search column is not executable path authority. Escape control characters/ANSI/OSC in all terminal output and diagnostic exports.

## 4. Local SQLite model

Logical tables to refine with measured queries:

| Table | Key data |
| --- | --- |
| `schema_migrations` | Applied version/checksum/time |
| `volumes` | Stable identity, mount incarnation, capability flags, pool grouping |
| `scan_scopes` | Explicit roots, exclusion/config fingerprint, platform encoding |
| `scans` | Run ID, full/incremental mode, start/end, state, owner |
| `root_generations` | Root, staging/active generation, coverage/error state, successful reconciliation time |
| `entries` | Root/generation, parent relationship, raw name/path, type, identity, logical/allocation/time/flags |
| `directory_state` | Enumerated generation, dirty reasons, watcher epoch, reconciliation deadline |
| `coverage_issues` | Root-relative private path token/raw local path, typed error, counts/time |
| `findings` / `finding_targets` | Rule/evidence/catalog fingerprint, nonoverlapping targets, policy/freshness |
| `ignores` | Rule ID + exact path/subtree scope + optional volume, user-created time |
| `catalog_versions` | Active/previous digest, version, verification/activation state |
| `cleanup_operations` / `cleanup_items` | Plan fingerprint, action, rule/catalog/policy versions, intent/outcome/receipt |
| `volume_samples` | Capacity/available bytes, timestamp, pool/volume identity |
| `service_state` | Heartbeat, owner nonce, schedule/backoff/notification state |

Use indexes for `(root, generation, parent)`, native identity, rule/finding scope, freshness, and size-sorted candidate pagination. Store only needed metadata; no content hashes in ordinary entries. History references snapshots of explanations/action data so a future catalog does not rewrite what the user approved.

### Transaction and generation rules

1. Acquire exclusive scan ownership before starting or recovering a scan.
2. Create `running` scan and staging state durably before traversal.
3. Ingest bounded batches into staging. Never modify the last valid published view in place merely to save a second generation.
4. Publish a completed root generation and its coverage together in a short transaction.
5. A partial root may publish an explicitly labelled partial view, but cannot advance `last_successful_reconciliation`. Keep the previous complete snapshot separately until retention permits pruning.
6. Delete missing cached entries only after their parent directory was successfully reconciled; permission/mount failures do not prove absence.
7. Interrupted runs become `interrupted`, retain truthful coverage, and require reconciliation. Staging generations never appear as complete.
8. Prune old/staging data in bounded maintenance batches after publication. Preserve current, last complete, and necessary audit/preferences. Do not retain whole scan copies indefinitely.

Start with a straightforward staging-generation design. Optimize later using copy-on-write/changed-subtree versions only if publication correctness, storage overhead, and query complexity are measured.

## 5. Scanner lifecycle

```text
validate scope -> acquire owner -> record running
     -> probe roots/mount/cloud capabilities
     -> establish invalidation epoch if available
     -> bounded traversal and metadata batches
     -> drain pending dirty scopes / report unresolved churn
     -> publish root views + coverage -> release owner
```

Scan controls include worker ceiling, per-volume concurrency, open-directory ceiling, queue bytes, database batch bytes, cancellation, and wall-time/work budgets. Begin conservatively and tune using benchmarks; CPU count alone is not an I/O policy. Backpressure must bound work inside the traversal backend, not just the SQLite channel. A blocked metadata syscall may not be immediately cancellable; report “stopping” and stop scheduling new work.

Normalize overlapping roots, identify mount/bind aliases, and avoid repeating physical inventory. Deny recursion into sockets/devices/pseudo filesystems and redirection. Known sensitive storage may be skipped completely or counted as a protected opaque entry; distinguish intentional exclusions from access errors.

### Incremental trust

Maintain dirty scopes and watcher epochs, not a boolean “unchanged” derived from mtime.

- A trustworthy cache shortcut requires prior successful enumeration, unchanged scope/exclusion/policy contract, unchanged root/mount identity, healthy event coverage for the interval, no dirty scope, and a reconciliation deadline not exceeded.
- Even then it is a freshness optimization, not deletion evidence; display when the subtree was last actually reconciled.
- Portable restart/offline gaps invalidate event continuity unless a separately tested native replay adapter proves otherwise. Re-enumerate affected roots.
- Overflow/backend errors/watch exhaustion/rename ambiguity invalidate the broader affected root. In-memory queue overflow records a persistent invalidation rather than silently dropping truth.
- Changing directory children can update directory mtime; rewriting an existing descendant often does not. Reconciliation must catch both.
- For large trees under ongoing writes, bound repeated reconciliation and publish partial/changing coverage rather than claiming a snapshot of a frozen filesystem.

## 6. Storage accounting

Keep the following separate:

| Quantity | Derivation |
| --- | --- |
| Volume used/available | Native volume/pool samples; permissions/quota semantics labelled |
| Scoped inventory | Unique observed identities within successfully covered roots |
| Directory subtotal | Derived rollup of descendant identities, never additive with nested subtotals |
| Candidate estimate | Union of target identity sets, with shared/link/allocation uncertainty |
| Selected estimate | Union of eligible selected targets, recomputed immediately |
| Successfully processed | Per-target confirmed handler outcomes, not guaranteed freed bytes |
| Observed available-space delta | Signed `available_after - available_before` per unique volume/pool, time-labelled and attribution uncertain |

Hard links: deduplicate by native identity in global totals, while allowing path-local displays to indicate shared ownership. Avoid an unbounded in-memory identity set by indexing identities in SQLite. If selected targets cover fewer links than reported link count, do not claim those blocks are reclaimable. If link count/identity is unavailable, state the limitation and avoid definitive unique-byte claims.

Allocation: Unix `st_blocks` uses 512-byte units, independent of filesystem block size; Windows allocation/streams require native probing. Sparse/compressed files need separate apparent and allocation values. Directory metadata blocks may contribute to accounting once, but directory length is not a descendant total.

Reflinks, APFS clones, snapshots, compression, alternate streams, and pooled volumes mean per-file allocation may not equal exclusive physical bytes. Do not introduce expensive extent enumeration in ordinary scans without a specific opt-in contract. Unknown/shared blocks remain uncertainty.

## 7. Process coordination

SQLite serialization is not scan or cleanup ownership.

- Use OS advisory locks in a validated private local state directory: one scan owner and one cleanup owner per profile. Locks release on process death; a PID alone is insufficient because it can be reused.
- Record owner nonce/heartbeat for diagnostics, not as a substitute for the kernel lock.
- CLI joins or requests cancellation of an existing scan rather than duplicating it. Foreground refresh has priority over future scheduled work.
- Cleanup holds its own exclusive operation lock and excludes simultaneous mutation of targeted scan scopes; independent filesystem writers still require native revalidation.
- Future service control uses a local Unix socket/Windows named pipe, peer identity/ACL checks, bounded versioned messages, and no network listener.
- No service cleanup RPC, no persistent authorization token, no replayable plan file granting permission.
- Migrations, corruption recovery, and profile reset require exclusive maintenance ownership; old clients reject a newer unsupported schema/protocol clearly.

## 8. Recovery and maintenance

See [operations.md](operations.md) for the user workflow. Technical rules:

- Distinguish transient busy/disk-full/I/O errors from corruption before quarantining anything.
- Preserve DB/WAL/SHM consistently under exclusive ownership or use backup API; never manually delete a live WAL.
- Keep user ignores/history separate logically from disposable inventory; recovery should preserve them when readable and never invent operation outcomes.
- Unresolved filesystem mutations are `interrupted/outcome_unknown`, not successful or retried. A missing original path alone is not a Trash receipt.
- Bound coverage-error storage by grouping repeated typed errors, with private detailed records when needed.
- Checkpoint and prune during idle work; monitor DB/WAL size and remaining disk space. Maintenance is never a catalog-driven arbitrary delete.
