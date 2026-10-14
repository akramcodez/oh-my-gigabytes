# Linux release checklist and decision (WP15)

Decision as of 2026-10-15: **not a release candidate. Not published.** Linux remains a development preview. The gates below are the evidence each advertised feature needs before it can be called supported.

## Decision by capability

| Capability | Shipped as | Why |
| --- | --- | --- |
| Analysis (scan, index, browse, findings) | Development preview, Linux only | Tested on tmpfs and Btrfs here. ext4, WSL2, ARM64 and oldest-kernel evidence missing (WP12) |
| Permanent delete (D15, home-only) | `development-unqualified`, enabled | Owner decision (D16). Race barriers and rollup oracle pass on tmpfs. Not qualified on any native profile. Not recoverable |
| OS Trash | Disabled (`unsupported`) | Race and restore semantics unresolved (`linux-os-trash-investigation.md`) |
| Scan-only service | Development, opt-in | Desktop notification path broken on the test host (`notify-send` library error). Second-user IPC test missing |
| npm/binary distribution | Not published | No production signing key or publisher identity; `artifacts.yml` never run remotely |

An analysis-only beta is possible once the owner decides to publish.

### Analysis-only build (built 2026-10-15, not published)

- Build: `cargo build --release -p omg-cli --no-default-features --locked`. The `permanent-delete` Cargo feature is off; the default build keeps it on.
- What it does: scan, browse, findings, dry-run preview, history, doctor, service status. Permanent delete is refused in the backend and hidden in the TUI. Doctor reports `permanent delete: unsupported`.
- Tests: default suite and analysis-only suite both pass (`cargo test --workspace --no-default-features`).
- Checksum (local copy, outside the repo): `sha256 5fdc6e4c3c4f64b5b0df0fff19f68fb0799d03d0866065add07ed4859462cd4c`.
- Not done for the beta: the package manifest still declares `cleanup: development-unqualified` for Linux. `assemble-release.js` and `verify.js` must be taught the analysis-only capability before any packaging. The delete code is still compiled into this binary, but it is unreachable through the backend gate and the UI.
 A full cleanup release is not, because no permanent-delete profile is qualified.

## Gate status

| Gate | Plan WP | Status |
| --- | --- | --- |
| Docs match code and manifest | WP01, WP02 | Done; TUI help and doctor show the same capability |
| Baseline recorded | WP03 | Done for this host; ext4 and ignored Btrfs re-run missing |
| Exact, one-use confirmation | WP04 | Done. Auto-fill default changed to **off** (decision made in this session; reversible in the menu) |
| Target and descendant safety | WP05 | Deterministic barrier tests pass on tmpfs (replacement, symlink swap, protected child). Birth-time fallback still a residual risk. ext4 and Btrfs native cases missing |
| Durable per-target outcomes, cancellation | WP06 | Done in core: intent in one transaction, per-target outcomes, interrupted labelling, cancel flag. TUI cannot cancel a running delete (single worker thread); the mechanism is tested but not wired to a key |
| Inventory correct after mutation | WP07 | Hard-link totals match an independent fresh scan. Partial removals flag a rescan. Crash, disk-full and corruption tests missing |
| Findings and scaling | WP08 | Synthetic benchmark recorded (10,100 entries: cold 27 ms, warm 27 ms). Representative trees, RSS/FD measurement and tool-root overrides missing |
| OS Trash investigation | WP09 | Delivered as an investigation; capability kept disabled |
| UI clarity and outcomes | WP10 | Done: auto-filled items labelled, failed review undoes staging, cancel discards plan, capability on help screen. Missing: PTY and hostile-name tests for the new screens, real-session usability review |
| Service qualification | WP11 | Missing: notification delivery (broken here), second-user IPC, journald policy, idle-budget measurement, WSL |
| Compatibility matrix | WP12 | Written with only observed evidence. Other profiles missing |
| Reproducible artifacts | WP13 | `artifacts.yml` restricted to Linux x86_64 GNU. Not run remotely. Actions not pinned by SHA |
| Upgrade, trust, governance | WP14 | Governance doc and `SECURITY.md` written. Owner items: reporting contact, signing key, publisher. Migration tests 001–011 missing |
| Release decision | WP15 | This document |

## Checklist before any publication (all owner-gated)

1. Native ext4 and WSL2 runs of the full suite, recorded in `linux-qualification.md`.
2. Race and failure suites for each enabled capability on each profile claimed.
3. Second-user and desktop-session checks for the service.
4. SHA-pinned actions, validated SBOM, advisory and license review.
5. Owner-provided signing key, publisher identity, security contact.
6. Explicit owner authorization to publish. Not granted in this session.

## Explicitly not claimed

- No profile is qualified. No platform is certified.
- Permanent delete is not recoverable and does not guarantee freed space on Btrfs or WSL.
- Notifications are not verified end to end.
