# oh-my-gigabytes

OMG (Oh My Gigabytes): local-first storage intelligence for developers, with a Rust engine and keyboard-first terminal UI.

The project is a development preview. Start with the [documentation guide](docs/README.md) for the product brief, research, implementation evidence, architecture, safety policy and acceptance criteria.

Track the [phase checklist](docs/checklist.md) and [Phase 0 evidence](docs/phase0.md). The Rust `omg` development build now provides scoped Linux metadata scanning, a SQLite index and read-only TUI. See [Phase 1 evidence and limitations](docs/phase1-evidence.md). Cleanup is disabled.

Phase 0 research and safety design is complete under the [owner-approved evidence review](docs/phase0-evidence-review.md). Native platform certification and independent developer validation remain explicit later gates.

```sh
cargo run -p omg-cli -- scan --root /path/to/local/linux/root
cargo run -p omg-cli -- status
cargo run -p omg-cli
```

Use `--state-dir /private/local/path` for a separate profile. Scanning currently requires Linux with `openat2` and an ext-family/tmpfs root; Windows/macOS adapters are not certified. Source code and independently authored catalog are licensed under MIT OR Apache-2.0.

Catalog findings and read-only review are available through the TUI or `omg findings`. Preview explicit IDs with `omg cleanup --dry-run --candidate ID`; inspect local preview history with `omg history`. **Actual cleanup is disabled** until native Trash race-safety qualification. See [Phase 2](docs/phase2-evidence.md) and [Phase 3](docs/phase3-evidence.md) evidence for unpassed gates.

Phase 4 now has private signed npm/offline packaging and a native artifact CI design. See [release instructions](docs/release.md) and [Phase 4 evidence](docs/phase4-evidence.md). No public npm package/signing key is provisioned and cross-platform release certification remains incomplete.
