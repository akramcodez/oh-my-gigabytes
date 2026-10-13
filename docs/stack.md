# Technology stack and dependency policy

Baseline: 2026-10-13. This is the recommended stack, not an installed dependency list. Snapshot versions and verified sources appear in [research.md](research.md).

## Core choices

| Concern | Recommended choice | Why / constraints |
| --- | --- | --- |
| Engine | Stable Rust, edition 2024 | Shared typed policy/accounting core; native OS integration; pin toolchain and establish MSRV from actual dependencies |
| CLI | `clap` | Consistent subcommands, typed inputs, help/completions; set executable name `omg` explicitly |
| TUI | `ratatui` + compatible `crossterm` backend | Responsive keyboard interface and terminal events; restore terminal with RAII/panic handling |
| Traversal | `dua-core` evaluation behind `ScannerBackend` | Maintained successor recommended by jwalk upstream; must pass boundedness/native-safety admission |
| Reference traversal | `walkdir` in tests/benchmarks | Simple correctness baseline; not assumed parallel production solution |
| Metadata | Rust standard APIs plus `rustix`/native Windows and Apple bindings where required | No-follow identity/allocation/cloud/boundary behavior needs platform-specific contracts |
| Index | `rusqlite` with bundled, patched SQLite | Ship predictable SQLite without end-user system development libraries; verify actual bundled version |
| Serialization | `serde`, strict `toml`, `serde_json` for bounded structured interchange | Reject unknown rule/action fields; escape paths separately from display |
| Background invalidation | `notify`, later milestone | Advisory dirty hints; no content polling; periodic reconciliation always available |
| Concurrency | Dedicated OS threads + bounded `crossbeam-channel` | Scanner and SQLite are blocking; simpler lifecycle and backpressure than an initial async runtime |
| Errors | `thiserror` in libraries; contextual CLI error wrapper if needed | Structured outcomes/redaction; distinguish unsupported, stale, denied, skipped, failed |
| Logging | `tracing` with bounded rotating local sink | No default raw filenames, command output, or inventories in diagnostic logs |
| OS data directories | `directories` or `directories-next`, selected after review | Platform conventions plus validation that state is local and private |
| IDs/time | `uuid`/OS randomness and a reviewed time crate | Random operation IDs, UTC audit times, monotonic in-process deadlines |
| Trash | Audited `trash` behind `TrashAdapter`, or native equivalent where required | Convenience is conditional on race safety, no permanent fallback, and native outcome visibility |
| Signatures | Reviewed `ed25519-dalek` + `sha2` | Signed exact-byte manifest and SHA-256 payload digests; strict verification and domain-separated formats |
| Optional duplicates | `blake3` + bounded opt-in pipeline; reevaluate engines later | No hashing in ordinary scans; duplicate matching never overrides protection |
| File manager | Reviewed native platform invocation | Fixed executable/API and argument vector; never arbitrary shell/URL execution |
| npm wrapper | Small handwritten JavaScript using Node built-ins | Spawn verified packaged native binary with inherited stdio; no native addon, compiler, web UI, or JS storage engine |

Do not add an async runtime solely because filesystem work is concurrent. If network catalog fetching later justifies one, keep blocking traversal/SQLite/native adapters off the UI thread and bound all queues.

## Walker admission decision

`jwalk` is no longer the default because upstream marks it unmaintained. `dua-core` is a strong **candidate**, not an unconditional decision. Require:

1. No-follow behavior for roots and all ancestors, not just encountered symlinks.
2. Native Windows reparse/cloud flags before directory descent and safe treatment of unknown tags.
3. Inspectable, enforceable worker, open-directory, queue-byte, and batch bounds.
4. Cancellation that stops submission, releases blocked producers, and joins workers without deadlock.
5. Per-root completion/error events and filesystem-boundary control.
6. Lossless path representation and correct identity/allocation metadata or hooks for augmentation.

If any essential capability is absent, contribute a narrow upstream improvement or implement an OMG-owned bounded backend over reviewed native enumeration. `ignore` is an alternative to benchmark, but disable implicit Git ignore semantics: ignored developer files still consume space and may contain valuable data. Do not maintain multiple production walkers without a demonstrated platform need.

## SQLite settings and build constraints

- Verify maintained SQLite **>= 3.51.3** as the preferred minimum WAL fix baseline; recheck later advisories. `rusqlite` version alone is insufficient evidence.
- Local private filesystem only. Validate directory ownership/ACL and symlink/redirection before opening application state.
- `foreign_keys=ON`, verified WAL activation, bounded busy timeout, explicit migrations, prepared/parameterized SQL.
- Start with `synchronous=FULL` for durable cleanup intent/history. Benchmark before changing durability; a cache optimization must not silently weaken audit guarantees.
- Short read transactions and bounded write batches; checkpoint on the writer owner; avoid checkpoint starvation.
- No dynamic SQLite extension loading or SQL supplied by catalog/config.

Use SQLite backup APIs for consistent backups. Never copy only a live DB file while ignoring its WAL.

## Platform support proposal

All rows are **targets for validation**, not current support promises. Final OS versions, deployment targets, libc baselines, and hardware certification belong in the release matrix.

| OS family | Initial architecture target | Filesystem baseline | Cleanup/service qualification |
| --- | --- | --- | --- |
| Linux | x86_64 GNU; aarch64 GNU after native testing | Local ext4 first; other local filesystems explicitly tested | Freedesktop-compatible Trash; systemd user manager when available |
| macOS | aarch64 and x86_64 | Local APFS | Native Trash, privacy permissions, cloud exclusion; per-user launchd |
| Windows | x86_64 MSVC | Local NTFS | Recycle Bin, file identities/reparse cases; per-user Task Scheduler |

Linux musl builds and Windows ARM64 are useful extensions after native tests. WSL is its own capability profile; do not assume Windows filesystem behavior or Linux service availability. Network filesystems, pseudo filesystems, unsupported cloud providers, and removable-volume cleanup are outside initial certification.

Volume-level capacity must respect shared APFS containers, mount aliases, and other shared storage pools. Do not sum multiple views of the same pool as independent physical capacity.

## Build and repository structure

Start with three crates and modular directories; split crates only when dependency or safety boundaries justify it:

```text
Cargo.toml                 # workspace
Cargo.lock                 # committed binary dependency lock
rust-toolchain.toml
crates/
  omg-core/src/            # model, scanner, index, rules, policy, planning
  omg-platform/src/        # linux, macos, windows metadata/action adapters
  omg-cli/src/             # clap, TUI, foreground orchestration
catalog/
  bundled/                 # versioned reviewed TOML, manifests
  fixtures/                # synthetic positive and negative rules evidence
migrations/
tests/
  adversarial/
  integration/
benches/
packages/
  oh-my-gigabytes/          # public npm launcher
  platform/                # generated target packages
docs/
```

This tree is a proposed future layout. Platform adapter execution requires core-authorized capabilities; avoid a cyclic dependency by locating shared action/metadata types in a small model module or extracting a types crate when needed.

## Dependency admission and licensing

Before adding a dependency:

1. Inspect exact package license, source license files, feature-selected transitive graph, maintainers, release history, and unsafe/native surface.
2. Run advisory/license checks (`cargo audit`, `cargo deny` or maintained equivalents) against the actual lockfile; document justified exceptions with expiry.
3. Confirm Rust/Node/OS requirements and reproducible builds for target platforms.
4. Minimize optional features and inspect install/build scripts.
5. Give safety-critical crates explicit integration tests and source review; a clean advisory scan is not a safety proof.
6. Preserve notices and generate a release SBOM/license inventory.

The owner approved MIT OR Apache-2.0 for the Rust code and independently authored catalog; both license texts are included. Do not copy GPL catalog material into a permissively licensed distribution without a deliberate compatible licensing decision. Assets and subcomponents need separate inspection. The initial workspace pins Rust 1.95.0 and uses an OMG-owned Linux bounded worker pool rather than adopting an unverified walker; exact selected dependencies are in Cargo.lock.

## Deferred dependencies

- Duplicate engines (`czkawka_core`, `fclones`): evaluate when optional content analysis is actually planned. Current dependency breadth is unnecessary for Phase 1.
- Machine learning: no MVP dependency or architecture requirement.
- Generic shell/plugin engine: excluded; catalog capabilities remain fixed builtin identifiers.
- Generic cross-platform service crate: only adopt if it preserves native per-user privilege, environment, stop, and uninstall semantics.
- HTTP client: introduce with authenticated catalog update implementation; enforce size/time/redirect limits and documented proxy behavior.

## Developer checks once implementation exists

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
```

Also run platform safety suites, dependency review, catalog validation, launcher tests, and release install smoke tests as defined in [testing.md](testing.md). These are future workflow commands; no Cargo workspace exists at this documentation milestone.
