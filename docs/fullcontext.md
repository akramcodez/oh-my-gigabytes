# OMG (Oh My Gigabytes): Storage Intelligence for Developers

A cross-platform, local-first storage cleaner that finds wasted disk space, explains every recommendation, and leaves every deletion decision to the user.

## 1. Vision

omg is a keyboard-first terminal application for Windows, macOS, and Linux that helps users understand what consumes their storage and safely reclaim space.

It continuously maintains a local index of filesystem metadata, identifies known cleanup candidates, explains why each item may be removable, and lets users review and approve cleanup operations through an interactive terminal UI (TUI).

Unlike a conventional disk cleaner, omg prioritizes transparency, safety, and developer workflows. It does not promise performance improvements, assume that old files are unwanted, or silently remove user data.

Core principle: Find it, explain it, verify it, and let the user decide.

### Name and distribution

- Product name: OMG (Oh My Gigabytes)
- npm package: oh-my-gigabytes
- CLI command: omg

Install and launch the application with:

```sh
npm install --global oh-my-gigabytes
omg
```

The npm package is the installation and distribution entry point; the storage engine remains written in Rust. End users should not need a Rust toolchain. Publish compatible prebuilt binaries or platform-specific packages for Windows, macOS, and Linux. If the installer downloads a binary, it must verify the release's integrity and authenticity, provide useful failure messages, and document an offline or manual installation path. The CLI must consistently use omg in commands, documentation, logs, and examples.

### Product goals

- Make storage analysis fast enough to use regularly.
- Identify meaningful, potentially reclaimable space rather than merely listing large files.
- Explain every recommendation in plain English, including uncertainty and potential consequences.
- Make cleanup safe, explicit, auditable, and recoverable where technically possible.
- Support Windows, macOS, and Linux through a shared Rust core and platform-specific adapters.
- Keep filesystem inventories, decisions, indexes, and logs local to the machine.
- Prioritize developer tooling and build artifacts as a distinguishing capability.

## 2. Target users

Primary audience: Developers and power users whose machines accumulate package caches, build artifacts, temporary files, SDKs, test output, and development-tool data.

Secondary audience: General desktop users who need to understand disk usage without navigating many directories or trusting an opaque cleanup utility.

The initial release should prioritize developer workflows while maintaining an architecture that supports broader desktop cleanup later.

## 3. Core user experience

The user runs:

```sh
omg
```

The application immediately opens the TUI using the latest valid local index, then refreshes stale information in the background when necessary.

The main screen displays:

- Total capacity and currently available space for each supported volume.
- Last successful scan time and scan coverage.
- Estimated potentially reclaimable space.
- Space selected for cleanup.
- Recommendations grouped by category and ordered by size.
- A clear reason and safety tier for every recommendation.
- Any scan limitations, inaccessible locations, or outdated results that affect accuracy.

Users can inspect an item, preview a cleanup operation, select or deselect candidates, open locations in the file manager, filter results, or permanently ignore a recommendation within a defined scope.

Before executing a cleanup, the application presents a confirmation containing the item count, paths or clearly scoped groups of paths, estimated size, action type, and relevant warnings.

No cleanup occurs without an explicit user-initiated action and the required confirmation. Selection, including preselection of eligible Safe-tier items, never authorizes deletion by itself.

Afterward, the application reports successful operations, skipped items, failures, and the observed change in available storage. It must not equate the sum of file sizes with the amount of space actually freed.

## 4. Functional requirements

### 4.1. Filesystem scanner

Build a fast, parallel, metadata-only scanner using Rust.

#### Required capabilities

- Scan accessible local filesystems within an explicit, documented scope.
- Enumerate paths, entry types, file sizes, timestamps where available, filesystem identities, and other metadata required for analysis.
- Use bounded parallel traversal to balance performance against system resource consumption.
- Maintain a local SQLite index for incremental updates.
- Avoid re-enumerating unchanged subtrees whenever their cached state can be trusted.
- Support full rescans, cancellation, progress reporting, and recovery after interruption.
- Handle permission errors, deleted files, renamed directories, inaccessible mounts, and other filesystem errors without treating partial scans as complete.
- Respect filesystem boundaries and configurable exclusions.
- Never follow symbolic links by default. Explicitly handle Windows junctions, reparse points, and equivalent filesystem redirection mechanisms.
- Never read file contents during the ordinary metadata scan.
- Never trigger downloads of online-only cloud files or read their contents for size analysis.
- Report unsupported filesystems, inaccessible locations, and incomplete coverage.
- Adapt worker counts and I/O usage to avoid excessive resource consumption.

The scanner must distinguish between apparent file size and allocated disk space wherever the platform exposes the necessary information. It must avoid double-counting hard-linked files and nested directory totals.

Filesystem notifications may be used to invalidate cached entries, but they must not be treated as a complete source of truth. Dropped events, directory renames, restarts, and database inconsistencies must be addressed through periodic reconciliation and a reliable full-rescan mechanism.

#### Performance requirements

- Reuse the last valid index for fast startup.
- Begin displaying useful results before a complete refresh finishes when cached data is available.
- Bound memory use, worker count, open file descriptors, and queued filesystem work.
- Benchmark cold scans, warm startup, and incremental refreshes against representative machines and directory trees.
- Report scan freshness and coverage rather than hiding incomplete work.

### 4.2. Local storage index

Use SQLite as the persistent metadata store.

The index should track only information required for storage accounting, candidate discovery, change detection, and cleanup safety.

It must include:

- A versioned schema and migration mechanism.
- Scan timestamps and per-volume scan status.
- Filesystem metadata and directory relationships.
- Cached rule-evaluation results where appropriate.
- Local ignore preferences and cleanup history.
- Recovery behavior for interrupted scans and database corruption.
- Safe coordination between the CLI and background service.
- Index pruning and maintenance to prevent unbounded database growth.

The index is a cache, not proof that a file still exists or remains safe to remove. Every cleanup target must be revalidated before an operation begins.

### 4.3. Rules catalog

The rules catalog is the primary mechanism for identifying known cleanup candidates.

Store platform-specific rules in versioned TOML files. Common rule definitions and shared validation logic should be reused where practical.

Each rule must define:

- A stable rule ID and human-readable name.
- Supported operating systems and applicable application or tool versions.
- Category and safety tier.
- A discovery strategy, including permitted path patterns or structured adapter identifiers.
- Required evidence for a recommendation.
- A plain-English explanation of what the item is and why it may be removable.
- Potential side effects, regeneration behavior, and relevant warnings.
- An approved action type: Trash, a dedicated cleanup adapter, or informational only.
- Preconditions, privilege requirements, exclusions, and failure behavior.
- Rule version, test fixtures, compatibility status, and deprecation information.

#### Initial categories

- Application and package-manager caches.
- Temporary files and application logs.
- Old installers and downloaded packages.
- Known, safely removable operating-system installation leftovers.
- Unused runtimes and development toolchains.
- Stale project build artifacts.
- Local iOS device backups on macOS.
- Old screenshots and downloads.
- Large, infrequently changed files.
- Duplicate files, introduced as an optional later-stage analysis.

Unknown files must not be classified as safe merely because their names contain terms such as cache, temp, or build.

Large files, old screenshots, unused runtimes, and stale projects generally belong in Review until a specific, well-tested rule establishes otherwise.

#### Catalog updates

The catalog must be updatable without requiring a new application binary.

Catalog updates must:

- Use authenticated or signed releases, with signature verification where supported by the release mechanism.
- Validate schemas, supported versions, path boundaries, and action definitions before activation.
- Reject invalid updates without replacing the last known-good catalog.
- Support rollback and offline operation.
- Record the active catalog version locally.
- Never grant new rules unrestricted shell execution or bypass protected-path policies.

The application should ship with a known-good catalog and continue operating safely when no network connection is available.

### 4.4. Safety tiers

Every cleanup candidate belongs to one of four policy outcomes.

#### Safe

A known, narrowly scoped item covered by an explicit, tested cleanup rule with understood regeneration behavior and no identified dependency that makes removal unsafe.

Eligible Safe items may be preselected, but the user must still explicitly review and confirm cleanup.

#### Review

An item whose value or consequences require human judgment. Examples include large untouched files, old installers, potentially stale projects, duplicates, old screenshots, and ambiguous application data.

Review items are unselected by default.

#### Info only

System-managed or otherwise non-actionable storage that should be explained but not directly deleted by omg. Examples include Time Machine snapshots and protected operating-system data.

Info-only items never expose a delete action.

#### Unknown or protected

Items without adequate evidence, items that fall within protected locations, and items that cannot be evaluated reliably must not receive a cleanup action.

The UI should explain when a candidate was excluded because of uncertainty or a safety policy.

### 4.5. Protected data and exclusions

Protection is enforced by the core application, independently of individual catalog rules.

The following are excluded from ordinary cleanup operations:

- Operating-system and application-critical system directories.
- Credential stores, keychains, private keys, authentication tokens, and secret-bearing configuration.
- Databases, database storage files, and live database-managed directories.
- Virtual-machine disks and their backing files.
- Docker storage directories, container volumes, and other container-managed storage.
- Git repositories and their internal metadata.
- Active downloads, partial installations, and files known to be in use by supported cleanup adapters.
- Cloud placeholders and any locations where scanning would trigger content downloads.
- Filesystem roots, home-directory roots, and protected parent directories as deletion targets.

Git-aware analysis may be added later, but identifying a repository as clean or pushed does not make the repository itself disposable. Protect uncommitted, untracked, ignored, stashed, worktree, and unpushed data. Do not delete repository directories as a shortcut for cleaning build artifacts.

Users may configure additional exclusions. User preferences must never weaken mandatory safety policies.

### 4.6. Interactive terminal UI

Use ratatui to build a responsive, keyboard-first TUI.

#### Main screen

- Storage overview by volume.
- Available space and scan freshness.
- Estimated potentially reclaimable space.
- Selected cleanup total.
- Items grouped by category and sorted by estimated size.
- Item name, location, tier, estimated size, and reason.
- Scan status, warnings, and coverage limitations.

#### Required interactions

- Navigate items and categories.
- Select or deselect candidates.
- Inspect detailed explanations and potential consequences.
- Preview the exact planned cleanup operation.
- Open an eligible location in the platform file manager.
- Filter and search by category, tier, path, and size.
- Ignore a recommendation and review or remove ignore rules later.
- Refresh results or request a full scan.
- Cancel an active scan or cleanup where safe.
- Review cleanup history and individual operation outcomes.

The interface must provide a persistent keyboard legend, adapt to narrow terminal sizes, and remain usable without a mouse.

The selected-space total must update immediately when selections change. It must be clearly labelled as an estimate rather than a guarantee of physical disk space freed.

### 4.7. Safe cleanup execution

Use trash or an equivalent platform-specific implementation to move eligible ordinary files into the operating system's Trash or Recycle Bin.

Never silently fall back to permanent deletion when Trash is unavailable.

Before each destructive operation:

1. Revalidate the target's path, type, filesystem identity, and relevant metadata.
2. Verify that the target remains within the rule's permitted scope.
3. Check mandatory protected-path policies and rule preconditions.
4. Detect replacement, unexpected filesystem redirection, and relevant concurrent changes.
5. Display a confirmation of the action, target count, estimated size, and warnings.
6. Execute only through the approved action handler.
7. Record the outcome and report any failures.

Use platform-specific safe filesystem operations where necessary to reduce time-of-check/time-of-use races.

#### System-managed cleanup

System-managed data must never be deleted directly by omg.

Where an official system utility is supported, use a dedicated adapter with a fixed, reviewed command and validated arguments. Display the exact action, its effects, privilege requirements, and any limitations before execution.

Do not execute arbitrary shell commands supplied by a catalog file. Never run cleanup commands with elevated privileges merely to gain access to otherwise protected data.

If a command cannot be safely previewed or its consequences cannot be described, expose the relevant storage as informational only.

#### Dry-run mode and audit log

Provide a dry-run mode that evaluates the cleanup plan without changing the filesystem.

Record cleanup attempts and outcomes locally, including:

- Operation ID and timestamp.
- Applicable rule and catalog version.
- Target and action type.
- Estimated size before the operation.
- Whether the operation was completed, skipped, or failed.
- Error details needed for troubleshooting, with privacy-conscious logging.
- Observed storage changes where available.

Undo and restore may be added later. The product must not promise restoration when platform Trash behavior, file replacement, or an irreversible official command makes it impossible.

### 4.8. Background service

A background scanner should maintain the index without continuously burdening the machine.

Use native service integration where appropriate:

- Windows: Task Scheduler or a suitable per-user service mechanism.
- macOS: launchd.
- Linux: systemd, with support limited by the user's environment and service permissions.

The service must:

- Run with the minimum privileges necessary.
- Operate at low priority where supported.
- Perform incremental rescans and periodic reconciliation.
- Use resource limits, cancellation, and backoff after failures.
- Avoid duplicate work when the CLI is running.
- Never perform automatic cleanup.
- Notify only on meaningful state transitions, such as crossing a user-configured free-space threshold.
- Implement hysteresis and cooldowns to avoid repeated notifications when free space fluctuates around a threshold.
- Avoid exposing sensitive filenames or paths in notifications by default.
- Clearly report whether the service is running, stopped, or unhealthy.

#### CLI commands

```sh
omg                # Open the interactive TUI
omg scan           # Scan or refresh the local index
omg scan --full    # Request a full reconciliation scan
omg start          # Start the background service
omg stop           # Stop the background service
omg status         # Show index freshness and service health
omg doctor         # Diagnose configuration and installation problems
omg cleanup --dry-run  # Preview an explicitly supplied cleanup plan
```

Any non-interactive cleanup command must require an explicit target selection and confirmation. There must be no unattended cleanup mode in the product.

## 5. Accurate storage reporting

The application must distinguish between:

- Storage used: Observed filesystem usage, subject to platform and permission limitations.
- Potentially reclaimable: The estimated size of discovered cleanup candidates.
- Selected for cleanup: The estimated size of currently selected targets.
- Successfully processed: Items actually moved to Trash or processed by an approved adapter.
- Space freed: The observed increase in available storage after accounting for concurrent system activity where possible.

These values are not interchangeable.

Trash moves can fail, caches may regrow, hard links may share storage, snapshots can retain blocks, and deleted files may continue occupying space while held open by processes. Filesystem compression, sparse files, and storage virtualization also complicate size estimates.

Always label estimates, report errors, and avoid claims of guaranteed recovery.

## 6. Architecture and technology stack

Language: Rust.

### Recommended components

- jwalk or a suitable alternative for parallel directory traversal.
- notify for filesystem change notifications, combined with reconciliation.
- ratatui and a terminal input library for the TUI.
- trash with platform-specific adapters for safe file removal.
- rusqlite for the local storage index.
- blake3 for optional duplicate detection, not mandatory hashing during ordinary scans.

Use a modular architecture:

1. Scanner: Filesystem traversal, metadata extraction, coverage reporting, and change detection.
2. Index: SQLite schema, persistence, invalidation, reconciliation, and migration.
3. Rules engine: Catalog loading, validation, matching, and safety-tier evaluation.
4. Policy engine: Protected paths, operation authorization, and target revalidation.
5. Cleanup adapters: Trash integration and approved platform-specific operations.
6. TUI: Navigation, selection, previews, confirmations, and reporting.
7. Service: Background scanning, scheduling, notifications, and process coordination.
8. Platform layer: OS-specific paths, filesystem behavior, services, and integrations.

The policy engine must be authoritative: neither the TUI nor the catalog can bypass mandatory safety checks.

## 7. Security, privacy, and licensing

The product must be local-first by design.

- No accounts, telemetry, analytics, or remote filesystem analysis.
- No uploads of filesystem metadata, filenames, paths, or cleanup decisions.
- Network access is limited to documented catalog update mechanisms and is configurable.
- All filesystem indexes, logs, and preferences remain local.
- Diagnostic output avoids sensitive paths and filenames by default.
- Installation and removal of the background service are explicit and documented.
- Builds and dependencies are reviewed for security, maintenance quality, and license compatibility.
- The project publishes a threat model and a process for reporting security issues.

Before adopting an existing cleanup engine, rules database, or duplicate-finding crate, verify its current license, maintenance status, dependencies, and suitability for the intended use.

## 8. Testing and release acceptance criteria

The application is not ready for release merely because it works on the developer's machine.

### Safety

- No protected-path deletions in the defined adversarial test suite.
- No automatic deletion under any operating condition.
- Every destructive action requires explicit user authorization and confirmation.
- Replaced paths, symlink substitutions, unsupported targets, and stale candidates are handled safely.
- Failed or incomplete operations are reported accurately.
- No silent permanent-deletion fallback when Trash is unavailable.

### Scanner correctness

- Scan coverage and access failures are visible.
- Hard links and nested directory totals are not double-counted.
- Cloud placeholders are not opened or downloaded.
- Incremental scans recover from dropped notifications and interrupted execution.
- Warm startup, cold scans, and refresh operations are benchmarked against representative workloads.

### Reliability

- Interrupted scans do not leave a falsely complete index.
- SQLite schema changes are migrated safely.
- Database corruption has a documented recovery procedure.
- Concurrent CLI and service activity does not corrupt the index or duplicate destructive operations.
- Operation logs reflect actual outcomes.

### Platform support

- Safety invariants pass on every officially supported operating system.
- Platform-specific filesystem and Trash behavior is covered by integration tests.
- Installation, upgrade, service management, and uninstallation are documented and tested.

### Usability

- Each recommendation has a plain-English reason and an appropriate safety tier.
- Users can preview targets and consequences before cleanup.
- Scan limitations and uncertain estimates are clearly labelled.
- Keyboard navigation and essential controls work in common terminal sizes.
- The result screen separates attempted, completed, skipped, and failed operations.

Performance targets should be established after benchmarking representative machines rather than relying on arbitrary universal timings.

## 9. Implementation roadmap

### Phase 0: Competitive validation and safety design

- Compare the current implementation and activity of related open-source projects.
- Identify a defensible differentiator before duplicating existing functionality.
- Define mandatory safety invariants, supported environments, and initial exclusions.
- Build a threat model and an adversarial filesystem test plan.

### Phase 1: Scanner and local index

- Implement parallel metadata scanning.
- Build SQLite persistence, incremental refresh, and reconciliation.
- Add protected-path handling, scan coverage, cancellation, and error reporting.
- Implement storage accounting and performance benchmarks.

Exit criterion: Reliable, explainable storage analysis with no deletion capability.

### Phase 2: Rules catalog and safety tiers

- Define and validate the TOML schema.
- Add an initial set of well-tested, developer-focused cleanup rules.
- Implement tier assignment, exclusions, catalog versioning, and update validation.
- Add plain-English reasons and potential side-effect descriptions.

Exit criterion: Every candidate has traceable evidence, an appropriate tier, and a tested policy decision.

### Phase 3: TUI and cleanup execution

- Build the keyboard-first interface.
- Implement preview, selection, filtering, and opening locations.
- Add target revalidation, Trash-based cleanup, confirmations, dry-run, and audit logs.
- Test failures, concurrent changes, and interrupted cleanup operations.

Exit criterion: Users can understand, review, and safely execute supported cleanup operations without silent deletion.

### Phase 4: Cross-platform hardening

- Validate supported Windows, macOS, and Linux environments.
- Complete OS-specific path, Trash, and service handling.
- Add packaging, installation, upgrades, security checks, and documentation.

Exit criterion: All mandatory safety and reliability tests pass on each supported platform.

### Phase 5: Background service and proactive storage intelligence

- Add low-priority scanning and threshold notifications.
- Add storage growth tracking and trend reporting.
- Improve developer-tool coverage based on real-world feedback.
- Introduce optional duplicate detection and Git-aware project analysis.

Exit criterion: The service remains resource-conscious and improves user decisions without introducing unattended cleanup.

### Phase 6: Optional personalization

Only after the product has collected enough consented, meaningful feedback should a local recommendation-ranking system be considered.

Any future model may prioritize Review-tier suggestions, but it must never override protected paths, authorize deletion, or weaken safety policy.

## 10. Future enhancements

Potential additions, prioritized by demonstrated user value:

- Duplicate detection using a suitable existing Rust engine.
- Git-aware analysis of stale projects and generated artifacts.
- Per-distribution Linux adapters for apt, dnf, pacman, Snap, and Flatpak.
- Read-only inventory of Docker storage and virtual-machine usage.
- Per-application storage breakdowns.
- Cleanup history, restore support, and better recovery reporting.
- Weekly local digests and optional notifications.
- Storage-growth trends and explanations for sudden disk usage increases.
- A locally executed ranking model for Review-tier suggestions, if real feedback shows a need.

## 11. Explicit non-goals

omg will not:

- Delete files automatically or offer an unattended cleanup mode.
- Perform registry cleaning.
- Claim to speed up computers through unsupported optimization claims.
- Delete system-managed data directly.
- Treat old, large, or infrequently modified files as automatically disposable.
- Read the contents of every file to analyze disk usage.
- Upload filesystem inventories or cleanup decisions.
- Require administrator privileges for ordinary user-level scanning.
- Use a learned model as a substitute for deterministic safety policy.

## 12. Success criteria

The project succeeds when users can routinely answer four questions:

1. Where is my storage going?
2. Which space can I potentially reclaim?
3. Why is each item a candidate, and what could happen if I remove it?
4. What exactly will change if I approve this cleanup?

The product must deliver fast and accurate discovery, honest estimates, reliable cross-platform behavior, and a deletion workflow users can trust on their primary machine.

Product principle: Storage visibility should be comprehensive, recommendations should be evidence-based, and destructive actions should always remain under the user's control.
