# oh-my-gigabytes

OMG (Oh My Gigabytes): local-first storage intelligence for developers, with a Rust engine and keyboard-first terminal UI.

The project is a development preview. Start with the [documentation guide](docs/README.md) for the product brief, research, implementation evidence, architecture, safety policy and acceptance criteria.

Track the [phase checklist](docs/checklist.md) and [Phase 0 evidence](docs/phase0.md). The Rust `omg` development build now provides scoped Linux metadata scanning, a SQLite index and read-only TUI. See [Phase 1 evidence and limitations](docs/phase1-evidence.md). Cleanup status (2026-10-15): "OMG Trash" only saves paths; explicit **permanent** deletion of saved or selected items exists on Linux inside your home folder after a confirmed plan (development implementation, not qualified; nothing goes to the system Trash and it is not recoverable). The older `planning::execute` Trash path always denies.

Phase 0 research and safety design is complete under the [owner-approved evidence review](docs/phase0-evidence-review.md). Native platform certification and independent developer validation remain explicit later gates.

```sh
cargo run -p omg-cli -- scan --root /path/to/local/linux/root
cargo run -p omg-cli -- status
cargo run -p omg-cli
```

Use `--state-dir /private/local/path` for a separate profile. Scanning currently requires Linux with `openat2` and an ext-family, tmpfs, or Btrfs filesystem. Network and cloud mounts are not certified. Btrfs scans treat nested subvolumes as boundaries; shared snapshot/reflink allocation is unknown, so indexed bytes are not a measure of space a cleanup would free. Windows/macOS adapters are not certified. Source code and independently authored catalog are licensed under MIT OR Apache-2.0.

Catalog findings and read-only review are available through the TUI or `omg findings`. Preview explicit IDs with `omg cleanup --dry-run --candidate ID`; inspect local preview history with `omg history`. **OS Trash integration is not implemented**, and permanent deletion is not yet qualified for release (see the [Linux release plan](docs/linux-release-implementation-plan.md)). See [Phase 2](docs/phase2-evidence.md) and [Phase 3](docs/phase3-evidence.md) evidence for unpassed gates.

Phase 4 now has private signed npm/offline packaging and a native artifact CI design. See [release instructions](docs/release.md) and [Phase 4 evidence](docs/phase4-evidence.md). No public npm package/signing key is provisioned and cross-platform release certification remains incomplete.

Phase 5 adds an optional, scan-only background service (Linux systemd user manager only) and `omg trends`. Nothing is installed unless you approve it: `omg start` explains and asks first, `omg service uninstall` removes it and keeps your data. The service cannot clean up files. See [Phase 5 evidence](docs/phase5-evidence.md).
