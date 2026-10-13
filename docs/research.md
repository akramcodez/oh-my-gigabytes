# Internet research and implications

Research date: **2026-10-13**. Sources below were accessed on that date. Repository default branches and `latest` documentation can change; pin versions/commits during implementation. This is a documentation-level review, not a source audit, platform experiment, or benchmark.

## Executive findings

1. Fast disk traversal and developer-artifact discovery already exist. OMG must distinguish itself through persistent storage intelligence, evidence, explicit uncertainty, independent safety policy, and trustworthy outcome reporting.
2. `jwalk` now explicitly says it is unmaintained and points to `dua-core`. Evaluate `dua-core` behind an OMG-owned scanner interface, with boundedness and cloud behavior as admission gates.
3. `notify` documents lost events and backend limitations. Use it to invalidate cache, never to certify that nothing changed.
4. `trash` is useful plumbing but is not a race-safe authorization boundary. Its documentation also notes mount-query thread-safety concerns. Audit the exact selected version and the surrounding mount enumeration calls before use.
5. SQLite WAL supports concurrent readers and one writer, but requires local storage and checkpoint management. Its current documentation describes a WAL-reset corruption fix; select a verified patched SQLite build.
6. Prefer npm platform-specific binary packages rather than install-time downloads. Authenticity still needs an explicit trust model; checksums alone do not authenticate a release.

## Competitive landscape

| Project | Current inspected evidence | License observations | Implication for OMG |
| --- | --- | --- | --- |
| `dua` | Parallel disk usage, interactive navigation/deletion, `dua clean` finds developer artifacts and checks Git ignored/tracked state. API latest release observed: v2.45.1, 2026-09-30. [R1, R2] | MIT at repository level | Developer cleanup alone is not novel. Learn traversal/TUI patterns; do not inherit its deletion policy. |
| `dust` | Compact disk-usage visualization, allocated/apparent distinction, filesystem boundaries and hard-link deduplication. Latest release observed: v1.2.6, 2026-09-16. [R3, R4] | Apache-2.0 | Strong baseline for readable space accounting. Compare analysis on identical roots and options. |
| Czkawka / Krokiet | Duplicate and large-file tools, cached analysis, shared Rust core. Current README says GTK Czkawka 12.0 is the last GTK release, with Krokiet as successor; core manifest inspected at 12.0.2. [R5, R6] | Core manifest MIT; README distinguishes MIT CLI/GTK, GPL-3.0-only Krokiet/Cedinia, CC BY 4.0 media | Do not infer component licenses from GitHub's repository badge. Core brings extensive media/archive dependencies that do not fit the initial metadata-only engine. |
| BleachBit | Cleaner definitions, Preview then Delete workflow; latest release API observed v6.0.4, 2026-09-08. [R7, R8] | Source and cleaner definitions GPL-3.0-or-later | Good comparison for catalog breadth and explanation quality. Independently author OMG rules from upstream tool documentation unless license adoption is explicitly decided. |
| `fclones` | Inspected manifest: duplicate-finding/removal package 0.35.0 with multiple hash algorithms, Rayon, sled, and OS-specific dependencies. [R9] | MIT in package manifest | Later duplicate-analysis candidate only. This review did not establish its latest release date or ongoing maintenance; recheck before adoption. |

Recent releases are maintenance signals, not proof of security, responsiveness, or suitability. Do not copy another program's safety claims into OMG's acceptance criteria.

### Differentiation hypothesis to validate

“Open immediately to a local, coverage-aware inventory; understand why developer storage is a candidate; inspect exactly what would change; safely approve a reviewed plan; inspect a truthful local audit.”

Validate with developer sessions comparing OMG's prototype against `dua`, `dust`, and a catalog cleaner. Ask users to answer the four success questions from the brief. Record consented qualitative feedback locally or through explicitly volunteered issue reports; introduce no analytics. A failure to provide materially better explanations and repeat-use behavior should narrow the roadmap before expanding catalog coverage.

## Dependency findings

| Component | Snapshot inspected | Finding |
| --- | --- | --- |
| `jwalk` | 0.9.0 docs; upstream README | README explicitly “Unmaintained”; docs description says use `dua-core`. Not the default choice. [R10, R11] |
| `dua-core` | 4.1.0, MIT | Fixed worker pool, streaming roots, descent predicate, no-follow symlinks, errors as iterator items, drop stops/joins workers. Queue bounds and native cloud-redirection semantics still need source review. [R12] |
| `notify` | 8.2.0, CC0-1.0 | Documents network/WSL gaps, watch limits, editor replacement behavior, and event loss for large trees. Never enable content-comparison polling for ordinary scanning. [R13] |
| `rusqlite` | 0.40.2, MIT | Direct synchronous SQLite interface with transactions, interrupt and backup support. Suitable for a dedicated writer thread. Verify bundled SQLite version separately. [R14] |
| `trash` | 5.2.9, MIT | Cross-platform Trash, Linux Freedesktop convention, OS-limited restore APIs; documentation warns of interactions with non-thread-safe mount functions. Neither universal restoration nor handle-bound execution is established. [R15] |
| ratatui | Website displays 0.30.2 | Appropriate responsive Rust terminal UI library. Terminal input, lifecycle restoration, and operation policy remain application responsibilities. [R16] |
| `ed25519-dalek` | 3.0.0, BSD-3-Clause | Candidate for authenticated offline catalog/release manifests. A cryptographic primitive does not supply key rotation, rollback protection, or release governance. [R17] |

Snapshot versions are observations, not a ready-to-copy dependency lockfile. Reassess Rust minimum version, license expression, advisories, dependency features, and cross-platform behavior at admission.

## Filesystem and platform findings

### Linux

`openat2` can restrict resolution beneath an anchored directory and reject symlinks, magic links, and mount crossings; available since Linux 5.6. `O_NOFOLLOW` alone only protects the final component. These controls help acquisition but do not turn a subsequent path-based rename/Trash call into a handle-bound operation. [R18]

Freedesktop Trash defines per-user storage, per-volume conventions, atomic unique information-file creation, and rejection of unsafe Trash symlinks. Trashing and erasing are distinct, and same-volume moves usually retain storage. The standard permits some behaviors broader than OMG's policy, so OMG must still deny permanent deletion fallback. [R19]

### Windows

Reparse points implement more than symbolic links: junctions, mounts, storage virtualization, and filters. Classify native attributes/tags; unknown redirection must not be traversed or acted upon. [R20]

`FILE_ATTRIBUTE_RECALL_ON_OPEN` means opening may fetch remote data. `RECALL_ON_DATA_ACCESS` can indicate virtualized directory contents, so even directory enumeration can fetch data. A blanket “metadata-only means never downloads” claim is false. Inspect parent entries before descending and exclude cloud roots when no safe provider contract exists. [R21]

Task Scheduler supports user security contexts and low-privilege run levels. Use per-user interactive-token registration without storing a password; document session limitations and permission errors. [R22]

### macOS

Apple's FSEvents guide says events are advisory, periodic full sweeps are necessary, dropped/coalesced events require broader scans, and monitoring should begin before snapshot scanning to capture concurrent changes. Persistent volume/event identifiers can help, but portable `notify` must not be assumed to expose durable replay. [R23]

Apple recommends `launchd` for per-user agents. Use a LaunchAgent rather than a root daemon; recheck actual contemporary `launchctl`/plist behavior on the selected supported OS versions because the inspected guide is archived. [R24]

Cloud/File Provider nonhydrating metadata and race-safe Trash operations on current macOS require native spikes; the archived FSEvents guide does not establish those capabilities.

### SQLite

WAL requires cooperating processes on the same host with suitable shared-memory support and is unsuitable for network-hosted index files. Only one writer operates at a time. Long-lived readers can prevent checkpoint progress and grow WAL. [R25]

The inspected WAL documentation identifies a reset bug fixed in **3.51.3 and later**, with backports **3.44.6** and **3.50.7**. Prefer a currently maintained bundled version at or above 3.51.3, and assert runtime version/build provenance in CI and `omg doctor`. This is a minimum fix baseline, not a claim that all later builds are defect-free. [R25]

## Tool-rule evidence

- npm documents `_cacache` as opaque, content-addressable, and self-healing. Its default cache root can contain other material. Do not trash `~/.npm` wholesale; a clean adapter must explicitly target validated cache data, show offline-install consequences, and use supported version behavior. `npm cache verify` reads cached contents and belongs to an explicit adapter, not metadata scanning. [R26]
- pip documents `pip cache dir/info/list/remove/purge`, and notes that internal layout can change between versions. `purge` affects HTTP and wheel caches, while `remove` addresses wheels. Prefer declared/versioned adapters over broad historical-path assumptions; warn about redownloads and rebuilding. Interpreter execution is executable code and needs a trusted executable boundary. [R27, R28]

Initial catalog recommendation: informational discovery for package caches, Review for individually evidenced downloaded artifacts where policy allows, and no Safe classification until precise evidence and platform tests exist.

## Distribution findings

npm's `bin`, `optionalDependencies`, `os`, `cpu`, and `libc` fields support a thin launcher and platform-specific packages. Optional dependency omission or failure does not necessarily fail installation; the launcher must give a useful deterministic missing-binary error. [R29]

npm provenance links a package to its build/source, not to an assurance that the package is benign. Trusted publishing generates provenance automatically; verification tooling and exact client requirements should be refreshed for the release. Binary provenance must cover the actual binary as well as the wrapper. [R30, R31]

Prefer no postinstall downloader. Publish same-version platform packages, authenticated binary manifests, and manual archives. If a downloader is introduced, it must validate a signed manifest against a pinned trusted key before activation, restrict extraction paths, and fail closed offline or on verification failure.

## Source register

- **R1:** [dua repository and README](https://github.com/Byron/dua-cli)
- **R2:** [dua latest release API](https://api.github.com/repos/Byron/dua-cli/releases/latest)
- **R3:** [dust repository and README](https://github.com/bootandy/dust)
- **R4:** [dust latest release API](https://api.github.com/repos/bootandy/dust/releases/latest)
- **R5:** [Czkawka repository and component licensing](https://github.com/qarmin/czkawka)
- **R6:** [Czkawka core manifest](https://raw.githubusercontent.com/qarmin/czkawka/master/czkawka_core/Cargo.toml)
- **R7:** [BleachBit README and licensing](https://raw.githubusercontent.com/bleachbit/bleachbit/master/README.md)
- **R8:** [BleachBit latest release API](https://api.github.com/repos/bleachbit/bleachbit/releases/latest)
- **R9:** [fclones package manifest](https://raw.githubusercontent.com/pkolaczk/fclones/master/fclones/Cargo.toml)
- **R10:** [jwalk upstream maintenance notice](https://raw.githubusercontent.com/Byron/jwalk/main/README.md)
- **R11:** [jwalk documentation](https://docs.rs/jwalk/latest/jwalk/)
- **R12:** [dua-core traversal documentation](https://docs.rs/dua-core/latest/dua_core/)
- **R13:** [notify known problems](https://docs.rs/notify/latest/notify/)
- **R14:** [rusqlite documentation](https://docs.rs/rusqlite/latest/rusqlite/)
- **R15:** [trash documentation and mount-query warning](https://docs.rs/trash/latest/trash/)
- **R16:** [ratatui official documentation](https://ratatui.rs/)
- **R17:** [ed25519-dalek documentation](https://docs.rs/ed25519-dalek/latest/ed25519_dalek/)
- **R18:** [Linux openat2 manual](https://man7.org/linux/man-pages/man2/openat2.2.html)
- **R19:** [Freedesktop Trash Specification 1.0](https://specifications.freedesktop.org/trash/1.0/)
- **R20:** [Microsoft reparse points](https://learn.microsoft.com/en-us/windows/win32/fileio/reparse-points)
- **R21:** [Microsoft file attribute constants](https://learn.microsoft.com/en-us/windows/win32/fileio/file-attribute-constants)
- **R22:** [Microsoft task security contexts](https://learn.microsoft.com/en-us/windows/win32/taskschd/security-contexts-for-running-tasks)
- **R23:** [Apple FSEvents guide, archived](https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/FSEvents_ProgGuide/UsingtheFSEventsFramework/UsingtheFSEventsFramework.html)
- **R24:** [Apple LaunchAgents guide, archived](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html)
- **R25:** [SQLite WAL behavior and WAL-reset fix](https://www.sqlite.org/wal.html)
- **R26:** [npm cache documentation](https://docs.npmjs.com/cli/v11/commands/npm-cache)
- **R27:** [pip cache command](https://pip.pypa.io/en/stable/cli/pip_cache/)
- **R28:** [pip caching semantics and path/version caveats](https://pip.pypa.io/en/stable/topics/caching/)
- **R29:** [npm package.json fields](https://docs.npmjs.com/cli/v11/configuring-npm/package-json)
- **R30:** [npm provenance and trusted publishing](https://docs.npmjs.com/generating-provenance-statements)
- **R31:** [GitHub artifact attestations](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds)
- **R32:** [systemd official project documentation portal](https://systemd.io/)

Two legacy Freedesktop Trash URLs returned 404; R19 is the successfully retrieved replacement. The direct systemd resource-control manual returned HTTP 418; only the R32 portal was retrieved. Specific systemd settings in the operations plan require validation against installed manuals and actual user-manager capabilities.

## Research still required before release

- Source-level walker resource limits, root/ancestor redirection, mount aliases, cancellation, and native attribute behavior.
- Current Apple File Provider metadata APIs, APFS shared-container reporting, privacy permissions, Trash behavior, and supported deployment targets.
- Windows file identities, allocated bytes/streams, native Recycle Bin flags and failure paths, long paths, and handle-bound rename/claim mechanics.
- Catalog signing format/key governance, npm name/namespace ownership, and supported Node/npm versions.
- Official cleanup utility semantics for every tool/version proposed for action.
- Exact dependency graph/license/advisory audit and real-machine performance measurements.

These are explicit implementation gates; internet documentation does not substitute for their tests.
