# Linux release governance and trust (WP14)

Status: partly done. Items marked **owner** cannot be completed in the repository.

## Done in the repository

- Release manifest declares capabilities exactly, and `verify.js` rejects any mismatch with the runtime (`cleanup: development-unqualified` on Linux, `disabled` elsewhere). `release.test.js` checks it.
- Signed binary manifests use Ed25519 with a pinned `keyId` and the tests use synthetic keys only.
- Catalog import requires a signed payload. Import is disabled until a release verification key is provisioned (`catalog.rs`).
- CI permissions are `contents: read` in both workflows. The artifact job requests `id-token` and `attestations` only for provenance.
- Native artifacts are restricted to Linux x86_64 GNU (`artifacts.yml`). macOS and Windows stay compile checks (`check.yml`), as the plan requires.
- `scripts/sbom.js` produces a CycloneDX 1.5 dependency inventory from `Cargo.lock` (local inventory, not a validated release SBOM).
- License files `LICENSE-MIT` and `LICENSE-APACHE` are present.
- `SECURITY.md` describes scope and how to report.

## Gaps (not done)

| Gap | Owner or next step |
| --- | --- |
| Pin GitHub Actions by full commit SHA (`actions/checkout@v4`, `dtolnay/rust-toolchain@1.95.0`, `actions/setup-node@v4`, `actions/attest-build-provenance@v3`, `actions/upload-artifact@v4`) | Repository work: needs verified SHAs from the upstream repos. Not done because SHAs were not verified here |
| Schema-validate the SBOM and add license and advisory review (for example `cargo deny`, `cargo audit`) | Repository work; tools not installed here |
| Security reporting contact and GitHub private vulnerability reporting | **Owner**: enable the feature and provide a contact in `SECURITY.md` |
| Production release signing key, publisher identity, npm namespace | **Owner**. Never stored in the repository |
| Key rotation and revocation procedure, offline trust instructions | Repository doc work after the owner's key policy is decided |
| Catalog signing key governance and re-verification of stored signed catalogs after local tampering | **Owner** decides the key; the code can only verify |
| Migration tests from every historical schema (001–011) | Repository work (WP14), not started |
| Upgrade test with a registered service binary path | WP11 and WP14 |
| Signed rollback behaviour for state artifacts | Not started; documented as a limit |

## Threat-model limits to publish

- OMG does not protect against a process running as the same user that replaces its state, binary or trust root. Manifests authenticate bytes and declared identity, not safety.
- A signed package is not a safety certification.
