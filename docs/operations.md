# Installation, local state, services, and release operations

Status: proposed operational design; packages and commands are not published/implemented by this documentation work. Public identity: product **OMG**, npm **`oh-my-gigabytes`**, executable **`omg`**.

Implementation update: Phase 4 private signed packaging, offline install verification and read-only `doctor` diagnosis now exist. See [release.md](release.md) for actual commands and [phase4-evidence.md](phase4-evidence.md) for tested scope; the remainder of this document includes planned release/service behavior, not certification.

## 1. Distribution design

Preferred layout:

- Public `oh-my-gigabytes` npm package with a small Node launcher and explicit `"bin": { "omg": "bin/omg.js" }`.
- Exact same-version optional native packages filtered by `os`, `cpu`, and Linux `libc` where necessary.
- Prebuilt `omg`/`omg.exe` included in native packages; no install-time compiler, native addon, or binary downloader.
- Signed target/version/digest manifests distributed with the launcher/native artifacts; launcher verifies manifest and native bytes before first execution, and rechecks when the package changes. A cached verification must not ignore a replaced binary.
- Manual native release archives with signature, manifest, digest, build provenance, license notices and SBOM.

Platform namespace ownership and npm package-name availability have not been established; choose/reserve them before publication. Do not assume an example namespace belongs to the project.

Target installation remains:

```sh
npm install --global oh-my-gigabytes
omg
```

Node/npm are needed for the npm installation path; the manual Rust binary must run without Node or Rust installed. Document supported Node/npm and OS/native runtime versions based on clean-machine tests.

### Launcher contract

1. Determine platform/architecture/libc capability; resolve exactly the expected versioned package.
2. Verify signed manifest with a pinned release trust root, binary digest, target and application version.
3. Reject absent optional package, wrong target/version, tampering, unsupported runtime, or permission failure with an actionable message.
4. Spawn packaged native binary with inherited stdin/stdout/stderr and argv; no shell. Forward signals/exit status and test native terminal behavior.
5. Do not silently fetch a binary or search PATH for an unrelated `omg` replacement.

Optional dependencies can be omitted/failed by npm without install failure. Explain the missing package and manual/offline path rather than pretending installation delivered a usable native engine.

## 2. Integrity, authenticity, and trust

Checksums detect byte changes only when the expected digest is trusted. Authenticate the manifest with an embedded trusted release key; pin target/version/digest. Publish provenance for **actual binary builds**, not just wrapper packaging. npm registry integrity/provenance adds assurance but is not proof the software is harmless, nor automatic attestation verification of an external download.

The root wrapper/trust key itself comes from an authenticated npm distribution/user trust decision. A fully compromised npm publisher or account can replace that root; signatures are not a magical defense against compromise of all trusted release channels. Use restricted trusted publishing/OIDC, reviewed workflows, separate catalog/binary keys, least CI permissions, dependency locks and release reviews.

Sign exact bytes with a versioned domain-separated format. Record key IDs/fingerprints and publish a verified rotation/revocation procedure before release. Do not trust a key downloaded alongside the artifact without a prior trust chain. Offline verification must use already trusted keys/bundles. Sign the final binary/archive **after** any code signing/notarization changes the bytes.

### If a downloader is introduced later

Require explicit documented installation network behavior, HTTPS policy, bounded time/size, restricted redirects, signed manifest verification before extraction/activation, safe archive paths/no links, target/version checks, private staging, atomic activation, useful errors and manual/offline fallback. Never activate an unsigned binary, trust only an unsigned checksum list, or disable verification to “fix” an offline install.

## 3. Offline/manual installation path

The release must publish this complete workflow with real artifact names, trust fingerprints and verification commands:

1. On a connected machine, acquire the platform archive, signed manifest, trust-root verification material and provenance bundle from the official release channel.
2. Authenticate the signing key through the documented trusted channel; verify the manifest/signature and archive/binary digest before use.
3. Transfer the verified bundle to the offline machine and verify it again against the already trusted key; no online verification dependency is required for the signed manifest path.
4. Extract only safe regular artifact paths into a user-owned location, put `omg` on PATH, and run `omg doctor`.
5. For offline npm installs, provide a documented bundle containing the wrapper and matching platform package, or a local registry/cache workflow. The wrapper tarball alone with unresolved optional dependencies is not a complete offline installation.

Concrete commands depend on the admitted signing format/tool and release namespace; do not invent commands that cannot verify a real artifact. Manual archives are the primary offline fallback and avoid installer network/toolchain requirements.

## 4. Local state and privacy

Proposed conventions, resolved through reviewed native directory APIs:

| OS | Config | Inventory/history/catalog state |
| --- | --- | --- |
| Linux | `$XDG_CONFIG_HOME/omg/config.toml` (fallback `~/.config/omg/config.toml`) | `$XDG_STATE_HOME/omg` (fallback `~/.local/state/omg`) on supported private local storage |
| macOS | User `Library/Application Support/omg/config.toml` | User `Library/Application Support/omg` |
| Windows | `%LOCALAPPDATA%\omg\config.toml` | `%LOCALAPPDATA%\omg` |

Native/redirected home folders can be cloud/network backed. Validate state location and offer an explicit supported local-state configuration if necessary; do not open WAL on unsupported shared/cloud storage. Validate ownership/ACL, ancestor redirection, restrictive permissions and profile identity before creating files.

State contains versioned SQLite inventory, preferences/ignores, cleanup history, active/previous verified catalogs, locks, bounded logs, and recovery records. Scanned filenames are sensitive local metadata even though file contents are not collected. SQLite is not encrypted by default; rely on documented OS user protections/full-disk encryption rather than claiming encryption that is not implemented.

Default diagnostic output reports capability, root/error counts, freshness and versions without raw sensitive paths. Detailed local display/export is explicit. No automated uploads or telemetry. Bound log/history/trend/cache retention with visible user settings; pruning disposable inventory does not erase or rewrite unresolved operation records silently.

### Configuration contract

Strict versioned TOML: scan roots, additional exclusions, resource budgets, refresh/reconciliation scheduling, catalog network opt-in, notification thresholds and retention. Configuration cannot enable unrestricted commands, follow unsafe redirection for cleanup, remove mandatory protection, elevate operations, or authorize deletion. Adding/removing roots/exclusions invalidates affected inventory/finding scope.

## 5. Diagnosis and recovery

`omg doctor` should report binary/install target/version/signature state, runtime SQLite version, private local-state permissions, schema/catalog compatibility, volume/metadata/Trash capabilities, terminal support and service health. Default diagnostics are read-only and redacted. Separate actual remediation that modifies state or installs service into explicit user-reviewed operations.

### Interrupted scan

- Preserve last valid results; mark the run/root interrupted and stale.
- Reconcile affected roots under exclusive scan ownership.
- Never mark unenumerated/inaccessible descendants missing merely because they were not visited.

### Corrupt index

1. Stop or coordinate service/CLI owners and acquire exclusive maintenance ownership.
2. Distinguish busy, disk-full, permissions and I/O errors from confirmed corruption.
3. Preserve/quarantine a consistent DB/WAL/SHM set; use SQLite backup API where readable. Never manually delete a live WAL or copy only the main file during writes.
4. Recover readable ignores/history/catalog pointers separately and flag uncertain history; do not fabricate success.
5. Create a fresh inventory schema and full reconciliation after an explicit reset decision. Keep the corrupted evidence until documented retention/user removal.

### Interrupted cleanup

Persisted intent without known outcome remains `outcome_unknown/interrupted`. Reconcile original identity and native Trash receipt/staging evidence where available. Missing original path is insufficient proof of successful cleanup. Do not replay, trash a replacement, overwrite an original-location replacement during restore, or promise automatic undo. Expose recovery records and truthful manual guidance.

### Disk-full/maintenance

Stop new scan ingestion/operations safely, preserve published state and audit intent, and report the failure. Bound DB/WAL/log growth through short queries, checkpointing and bounded pruning. Database reset/maintenance is application-state management with explicit scope, never an excuse to clean user files automatically.

## 6. Background service design

Delivered in Phase 5 after foreground coordination is stable. No service installed by npm install or ordinary `omg` launch. `omg start` first explains and explicitly approves per-user registration if absent; subsequent starts use that registration. `omg stop` stops scheduled/active maintenance according to safe cancellation semantics; removal is a separate explicit documented command (proposed `omg service uninstall`).

| Platform | Integration | Constraints |
| --- | --- | --- |
| Linux | systemd user service/timer or user-managed scan process | No system service/root/automatic lingering; detect user manager/cgroup capabilities; unavailable means unsupported service, not app failure |
| macOS | User LaunchAgent | Current-user ownership, validated argv/absolute binary, contemporary launchctl behavior tested; no root LaunchDaemon |
| Windows | Per-user Task Scheduler | Interactive token/least run level, no stored password/admin escalation; logged-in-session and registration restrictions reported |

The service launches the native binary via a validated stable path, not through a project CWD/Node wrapper with changing PATH. Service install records version/path/owner; upgrades preserve a valid path or require explicit registration repair. Keep service artifact management separate from user cleanup.

### Runtime policy

- Scan-only orchestration; no cleanup API/RPC or automatic plan approval.
- Same kernel scan lock as foreground CLI; foreground requests preempt/coalesce scheduled scans safely.
- Low priority using native mechanisms; bounded workers/queues/descriptors, per-volume limits, timer jitter and failure backoff.
- Linux settings such as nice/CPU/IO/memory controls must be validated against actual user-manager capabilities; missing controls are visible and native budgets still apply.
- macOS settings use validated LaunchAgent/QoS controls; Windows low/background priority is tested on supported sessions.
- Idle/battery/active-work scheduling where reliably available; no invented universal low-load guarantee.
- Advisory watchers with overflow invalidation plus periodic full reconciliation and restart-gap recovery.
- Healthy/running, stopped, starting/stopping, degraded/unhealthy and unsupported states distinguished by native registration/process and heartbeat evidence, not only stale PID.
- Local authenticated IPC, bounded versioned messages, no listening network port.

### Notification state machine

Persist per-volume/pool user-configured low threshold, recovery threshold (`recovery > low`), minimum dwell and cooldown.

```text
normal -> below-low pending -> sustained-low notified
      -> recovered above recovery threshold -> normal
```

No repeated notification while already low; recovery/re-crossing still respects cooldown. Handle restart/clock changes with monotonic process deadlines and conservative persisted timestamps. Message includes only nonsensitive storage state, never filenames or cleanup targets. A notification opens analysis at user request; it never runs cleanup.

## 7. Upgrade and uninstall

Upgrade: verify new artifacts, preserve prior working binary/catalog, migrate DB under exclusive maintenance ownership, reject incompatible old clients, and update service registration only through the documented explicit mechanism. Keep app/catalog/schema versions independent and record actual active versions.

Uninstall:

1. Explicitly stop and remove registered per-user service/task first when installed.
2. Remove npm package through `npm uninstall --global oh-my-gigabytes`, or remove the manually installed native artifact.
3. Preserve local preferences/index/history by default; offer separately documented explicit local-state removal with exact scope.
4. Never empty Trash or delete scanned user data as part of uninstallation.

Do not rely on npm lifecycle uninstall hooks to stop services reliably. `doctor` should diagnose orphaned registrations and give accurate native/user-level removal instructions for the actual registered artifact.

## 8. Release pipeline

1. Review source/catalog changes, resolved safety gates and supported capability matrix.
2. Pin toolchain/lockfiles/workflows; minimize dependency features and audit license/advisory/build-script surface.
3. Native builds/test evidence for each target; static/native runtime linking decisions verified on clean machines.
4. Generate SBOM/notices and final artifacts; code sign/notarize where adopted; hash/sign final bytes.
5. Generate build provenance for actual binary outputs and npm packages with supported trusted publishing.
6. Verify signed artifacts independently, then assemble exact-version platform packages/launcher.
7. Install-test final tarballs/archives, including offline and tampering/missing dependency cases.
8. Publish platform packages before the wrapper version references them; prevent partial availability from being presented as a complete release.
9. Publish supported OS/features, known limitations, installation/recovery/service docs, verification keys/instructions and security reporting process.

Catalog-only releases use the same schema/evidence/protection validation and signed activation mechanism. A signed catalog cannot expand builtin capabilities or bypass policy. Final release acceptance is the checklist in [testing.md](testing.md).
