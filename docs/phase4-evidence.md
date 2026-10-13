# Phase 4 development evidence

Status: **packaging/authenticity development work implemented; full Phase 4 acceptance incomplete.** Phases 1–3 also retain documented acceptance blockers. No public package, service or destructive capability was enabled.

## Implemented

- Private `oh-my-gigabytes` npm launcher, fixed `omg` binary name, Node built-ins only, same-version platform-package resolution and self-contained offline packaging.
- Explicit-key signed assembly, domain-separated Ed25519 exact-byte manifests, SHA-256 size/digest checks, strict target/schema/version/capability validation, native executable header checks and per-launch verification.
- No postinstall downloader, native addon build or end-user Rust toolchain; missing optional package/unprovisioned keys/unsupported libc fail closed with useful messages.
- Offline/manual verifier with independently supplied public key, license inclusion, refusal to overwrite release output, no private key in generated artifacts.
- Diagnostic separation: `omg doctor` does not initialize or migrate state; reports existing index schema/integrity through read-only queries.
- Native development-artifact CI design for Linux GNU, Apple ARM64 and Windows MSVC with build provenance, license artifacts and local dependency inventory. Hosted jobs have not run here.
- Release/install/upgrade/uninstall guide and explicit unsupported-capability matrix.

## Verification on this environment

Environment: WSL2 Linux x86_64, Node 22.20.0, npm 10.9.3, Rust 1.95.0. Tests use synthetic keys/fixtures in temporary locations; no production keys, real user data cleanup or public publication.

- Rust workspace tests: 16 passing, including no-initialization/corruption/future-schema diagnosis checks.
- Clippy with `-D warnings` and whitespace checks passed.
- Node package tests: 10 passing at final check, including signed assembly, modified binary, authenticated wrong architecture, tampered signature, wrong key/target/version/scope/capabilities, oversize/unknown metadata, symlink denial, missing package, empty trust and offline install/reinstall/uninstall.
- Actual packed offline npm bundle installed under an isolated prefix containing spaces with `--ignore-scripts --offline`; verified binary `--version`, read-only `doctor`, scan, CLI failure propagation and user state preserved after uninstall.

Executable-header checks passed. Linux PTY restoration smoke passed. Local CycloneDX inventory generation parsed successfully with 170 components/dependency records; external schema validation, advisory review and full license review remain pending. Package tests are not Windows/macOS installs. The native artifact workflow is implemented but not executed here.

## Cross-target attempts

Rust std targets installed into temporary toolchain storage. `cargo check --target x86_64-pc-windows-gnu` failed in bundled SQLite's build script because MinGW C compiler was unavailable. `cargo check --target x86_64-apple-darwin` failed because host `cc` lacks Apple `-arch`/deployment-target support and no Apple SDK/toolchain is configured. These are **failed qualification attempts**, not successful cross-platform builds. Native CI is the correct next verification path; no system toolchain packages were installed or compiler errors bypassed.

## Release capability matrix

| Target | Packaging identifiers implemented | Native build/run evidence here | Analysis | Cleanup/service |
| --- | --- | --- | --- | --- |
| Linux x86_64 GNU / WSL2 | Yes | Development Rust binary and local offline npm install tested | Local ext-family/tmpfs development profile only | Disabled |
| Linux ARM64 GNU | Yes | Not run | Uncertified | Disabled |
| macOS ARM64/x86_64 | Yes | Not run; x86_64 cross check blocked by compiler/SDK | Disabled | Disabled |
| Windows x86_64 MSVC | Yes | Not built/run here; GNU cross check blocked by compiler | Disabled | Disabled |
| Linux musl/other targets | No | No | Unsupported | Disabled |

## Remaining Phase 4 gates

- Real native metadata/cloud/permission/Trash tests and independent developer usability validation; preceding-phase incomplete gates remain binding.
- Actual native CI builds, Windows/macOS installers/signals/runtime behavior and ARM64 tests.
- Public signing root/publisher/namespace ownership, key rotation/revocation, authenticated public artifacts and npm/binary provenance.
- Full historical upgrade/migration and disk-full/corruption recovery acceptance; signed rollback/hardening of state artifacts.
- Dependency security/license review, validated release SBOM/notices, pinned/reviewed workflow dependencies, clean-machine runtime ABI matrix.
- Private install directory/ancestor redirection/TOCTOU execution hardening; full diagnostic privacy review.
- Native code signing/notarization where adopted and explicit service integration feasibility/lifecycle.

The project remains a development preview. Phase 4 is in progress, not complete or release-ready. See [release.md](release.md) for exact implemented packaging commands and limitations.
