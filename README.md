# oh-my-gigabytes

OMG (Oh My Gigabytes): local-first storage intelligence for developers, with a Rust engine and keyboard-first terminal UI.

The project is currently in research and planning. Start with the [documentation guide](docs/README.md) for the full product brief, source-cited research, implementation plan, stack, architecture, safety policy, and phased acceptance criteria.

Track the [phase checklist](docs/checklist.md) and [Phase 0 evidence](docs/phase0.md). The Rust `omg` development build now provides scoped Linux metadata scanning, a SQLite index and read-only TUI. See [Phase 1 evidence and limitations](docs/phase1-evidence.md). Cleanup is disabled.

Phase 0 research and safety design is complete under the [owner-approved evidence review](docs/phase0-evidence-review.md). Native platform certification and independent developer validation remain explicit later gates.

```sh
cargo run -p omg-cli -- scan --root /path/to/local/linux/root
cargo run -p omg-cli -- status
cargo run -p omg-cli
```

Use `--state-dir /private/local/path` for a separate profile. Scanning currently requires Linux with `openat2` and an ext-family/tmpfs root; Windows/macOS adapters are not certified. Source code and independently authored catalog are licensed under MIT OR Apache-2.0.
