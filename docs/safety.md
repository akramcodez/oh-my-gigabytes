# Safety policy and threat model

Status: mandatory design baseline; implementation must prove it. [fullcontext.md](fullcontext.md) is authoritative. Test matrix: [testing.md](testing.md).

## 1. Mandatory invariants

| ID | Invariant |
| --- | --- |
| S01 | No cleanup without an explicit user-initiated operation and separate confirmation of its exact plan |
| S02 | Selection, Safe tier, ignores, cache freshness, service events, and saved plans never authorize deletion |
| S03 | Protected or uncertain targets have no cleanup action; policy outranks UI, rules, config, and adapters |
| S04 | Ordinary scanning reads metadata only and never intentionally hydrates cloud content |
| S05 | No default traversal through symlinks/junctions/reparse redirection, roots outside scope, or mount boundaries |
| S06 | Targets and relevant ancestors/descendants are revalidated at preview and immediately before execution |
| S07 | Ordinary cleanup uses Trash only; unavailable Trash never triggers permanent removal fallback |
| S08 | System-managed data has no direct delete operation; official adapters are fixed, reviewed, scoped capabilities |
| S09 | No arbitrary catalog commands, shell evaluation, automatic elevation, or executable project configuration |
| S10 | Audit intent/outcome reflects actual known state; interrupted cleanup is not auto-replayed |
| S11 | Estimated bytes, successful processing, and observed available-space change are separate |
| S12 | No inventory/decision uploads, telemetry, filename-bearing notifications, or automatic service installation |

No force flag, user exclusion override, or custom rule may weaken these invariants.

> **Current implementation note (2026-10-15, WP01):** S07 is the design baseline for *ordinary* cleanup. Owner decision D15 ([plans.md](plans.md)) adds an explicit, home-only **permanent** deletion flow (`cleanup::Index::delete_permanently`) on Linux and, since 2026-10-15 at the owner's request, Windows NTFS (delete by verified handle; see [windows-evidence.md](windows-evidence.md)). That flow is not Trash, is not recoverable, and is a development implementation without qualification. S02, S03, S06, S10 and S11 still apply to it. The older `planning::execute` Trash path still always denies. OS Trash is not implemented. See [linux-release-implementation-plan.md](linux-release-implementation-plan.md) for the qualification gates.

## 2. Protected data

Core policy combines native protected roots, exact sensitive components, positive rule scope, entry types, identities, and adapter evidence. A blocklist alone cannot classify all sensitive files.

| Class | Examples / required behavior |
| --- | --- |
| System/application critical | OS directories, installed application binaries/data needed for operation, protected parents; deny ordinary removal |
| Credentials | `.ssh`, keychains, `.gnupg`, credential managers, token/config stores, known secret files; exclude actions and avoid logging contents |
| Databases | Known storage roots, database files and sidecars, live managed stores; deny parent removal that contains them |
| VM/container storage | Virtual disks, snapshots/backing chains, Docker volumes/managed data; inventory only where safe |
| Repositories | Repository roots, `.git` directory/file, case variants as appropriate, bare repositories, nested/worktree metadata; no actions inside known repos initially |
| Active/transient work | Partial downloads, lock/in-use indicators, installation staging, supported adapter activity; deny on evidence or uncertainty |
| Cloud | Known provider roots, recall/offline/reparse placeholders and virtual directories; do not descend where metadata can trigger fetch |
| Root/parent targets | Filesystem root, home root, application state root, protected parents, volume mount root; never action targets |
| Unknown special entries | Devices, sockets, pipes, unsupported types/redirections or missing identity; no action |

User-configured exclusions only add protection. Ignoring a recommendation changes visibility, not authorization. Renamed secrets/databases cannot always be detected without content analysis; positive narrow evidence is essential and broad cache-parent removal must fail when contents cannot be adequately classified.

Inventory may show protected storage if metadata access is safe. It must label omitted subtrees and never suggest that reading them grants deletion permission.

## 3. Threat model

### Assets and trust boundaries

Protect user data, credentials, repositories, availability, truthful index/history, release authenticity, and local privacy. Untrusted inputs include filesystem names/topology, concurrent processes, rule packages before verification, cached metadata, user-edited config/index, executable discovery, terminal escape sequences, archive contents, IPC messages, and network responses.

Trust the reviewed application/policy build, its verified catalog trust roots, and OS primitives within tested capabilities. An attacker with the same account can often modify user-owned executables/state; OMG is not a security boundary against a fully compromised user account or kernel. It must still reject malformed plans, path substitutions, forged cached targets, and unsafe redirection rather than treating local files as inherently trustworthy.

| Threat | Required mitigation |
| --- | --- |
| Symlink/junction/ancestor substitution | Anchored native path resolution, parent identity checks, no-follow acquisition, reject unexpected redirection |
| Mount swaps/bind aliases | Mount incarnation/capability checks, no-cross-boundary operation scope |
| Target replaced or directory gains protected child | Fresh native validation; no parent removal without recursive safety evidence and safe acquisition semantics |
| Tampered catalog/cache | Verified signed catalog; strict schema/capabilities; cache never deletion proof |
| Malicious rule requests arbitrary command | Builtin action enums only, core scopes and protection independent of TOML |
| Malicious filenames | Native raw paths; sanitized terminal display; parameterized SQL; argv/native APIs, no shell |
| PATH/interpreter hijack | Explicit validated executable resolution/version, safe environment/cwd, no project-local tool execution by default |
| State-dir symlink/ACL abuse | Validate state location/ownership, private permissions, bounded files and authenticated local IPC |
| Crash after moving file | Durable intent, native receipt when possible, unresolved outcome state, user-visible reconciliation; no retry |
| Resource exhaustion | Bounded queues/descriptors/DB/logs; backpressure; stop scheduling on cancellation/disk-full |
| Release/download replacement | Pinned signing root, authenticated exact target/version/digest, provenance verification and safe extraction |
| Privacy leakage | Local storage, default redaction, no diagnostic uploads, opt-in detailed display/export |

## 4. Authorization flow

```text
explicit selection -> policy-filtered frozen plan -> preview
    -> explicit confirmation bound to fingerprint
    -> acquire operation lock -> durable intent
    -> immediate native revalidation/acquisition -> approved handler
    -> durable per-target outcome -> summary + volume samples
```

The preview validates path/type/identity, rule scope/version/evidence, protection, permitted action, privilege, coverage, and warning data. The fingerprint includes target raw paths and identities, action scope, versions, estimates, and warnings. Confirmation is session-local, one-use, short-lived, and never stored as reusable authority.

After confirmation, revalidate again for each target. Never silently add newly discovered targets to an authorized group. Material changes to action, target set, warnings, policy/catalog version, or identity invalidate the plan and require a new review. Disappeared/unsafe individual targets may be skipped and reported; increased or changed scope cannot be approved implicitly.

Directory operations require evidence for every descendant affected by moving the parent. Reject if enumeration is incomplete, there is an excluded/protected descendant, a nested mount/redirection, or concurrent change that cannot be safely contained. A parent timestamp check alone is inadequate.

## 5. TOCTOU and Trash: unresolved release gate

Calling `canonicalize`, `symlink_metadata`, or even `openat2`, then passing the pathname to `trash::delete` leaves a gap: the pathname can refer to a new object when the handler resolves it. Native handles help validate/acquire objects but do not automatically constrain a later path-based Trash API.

Required platform spike:

1. Anchor and validate the allowed scope and all ancestors using native identities/no-follow primitives.
2. Determine whether native Trash can consume a stable acquired object, or whether a safe claim/staging mechanism is needed.
3. If staging is investigated, record intent before mutation; claim into private same-volume staging using reviewed native semantics, then compare the claimed identity **before** Trash. Never treat ordinary rename as an atomic compare-and-swap against expected identity.
4. Test a substituted protected target, concurrent directory writers, open handles, cross-volume paths, and crash during staging. A wrong claimed object must never be trashed. Restoration must avoid overwriting a replacement at the original location; failure leaves an explicit recovery record.
5. Preserve native Trash metadata/restore usability; staging must not silently make the only recorded original location a private temporary path.
6. If safe semantics cannot be demonstrated for an environment/target type, expose it informationally and deny cleanup.

Staging is an investigation, not an approved general solution. It is itself a mutation, can move the wrong substituted object, and may not freeze descendant writers. Passing filesystem race tests is required before any implementation advertises directory cleanup. Linux path-relative resolution controls, Windows no-reparse handles, and macOS native acquisition all need independent certification.

Audit the chosen `trash` version's Linux mount enumeration and other libraries in the process for the documented non-thread-safe mount-query interactions. Serializing only OMG's own adapter calls is not sufficient if other code invokes incompatible libc APIs concurrently.

## 6. Dedicated utility adapters

No generic shell runner is exposed. Each reviewed adapter contains:

- Fixed action ID, supported platform/tool versions, trusted executable acquisition, argument construction, safe environment/cwd.
- Explicit target/scope and maximum effects, expected preconditions, activity detection, privilege requirements.
- Exact preview text, reversibility/irreversibility warning, timeouts and cancellation semantics, bounded output.
- Per-target or clearly group-scoped outcome interpretation and relevant postchecks.

The catalog can request only builtin capabilities and validated parameters. Executing a tool is code execution even for “cache info”; PATH, Python startup/customization, user config, and plugin loading require review. Do not run commands to infer dependencies during ordinary scans. If effects cannot be bounded/described, remain info-only. No elevated operation is added to gain access to protected data.

## 7. Failure and cancellation semantics

| Event | Behavior |
| --- | --- |
| Stale/missing/replaced target | Skip with typed reason; never retarget by filename |
| Unknown scope/identity/incomplete child inventory | Deny action; preserve informational finding |
| Trash unavailable/full/permission denied | Fail without permanent deletion fallback |
| Native API partially processes a group | Record individual known outcomes; do not call whole group successful |
| Cancellation before mutation | Stop; leave filesystem unchanged |
| Cancellation during an atomic native move | Finish/reconcile that step, then stop before next target |
| Utility cannot be safely interrupted | Show stopping/adapter limitation; do not imply rollback |
| Crash between intent and outcome | Mark unresolved on restart; reconcile without automatic cleanup |
| Audit storage fails before mutation | Do not begin the target operation |
| Audit storage fails after mutation | Stop subsequent work, surface outcome uncertainty and available recovery evidence |

Trash is not a universal undo promise. Automatic restoration or automatic replay must not occur merely because the application restarts.

## 8. Security reporting and release review

Before public release, publish an actual security contact/private reporting mechanism and tested supported-version policy. Do not invent an email address in documentation. Security reports should use synthetic reproductions where possible and avoid public sensitive filenames/logs. Review safety-critical changes, catalog promotions to Safe, new actions, release keys, and platform capabilities explicitly. Record findings and regression fixtures locally in the project.
