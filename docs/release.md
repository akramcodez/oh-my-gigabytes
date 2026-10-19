# Development distribution, authenticity and installation

Status: Phase 4 development packaging implementation. **No npm package or signed public release is published.** Source wrapper/native packages are private; the production signing key and npm namespace ownership are not provisioned. **Update (2026-10-19):** the first release is an analysis-only beta (`0.1.0-beta.1`) for Linux, Windows and macOS; see [Releasing the beta](#releasing-the-beta) and [beta-release-progress.md](beta-release-progress.md). Releases default to the analysis-only variant, in which permanent delete is compiled out. A full build still declares permanent delete as `development-unqualified` on Linux and Windows; OS Trash is not implemented.

## What exists

- `packages/oh-my-gigabytes`: Node 22+ launcher with `omg` bin, no runtime dependencies/download/install scripts.
- `scripts/assemble-release.js`: builds exact-version platform package, wrapper with optional dependency, and self-contained offline wrapper from an explicitly supplied binary and maintainer-controlled Ed25519 key pair.
- `scripts/verify-release.js`: offline verification using an independently trusted public key, expected target and version.
- `scripts/sbom.js`: CycloneDX 1.5 dependency inventory from locked Cargo metadata; validate against the CycloneDX schema and review licenses before production release.
- `.github/workflows/release.yml`: builds the analysis-only binaries for all five targets (Linux x64 and arm64 in a `manylinux_2_28` container, Windows with a static C runtime, macOS arm64 and Intel), checks them, attests their provenance and uploads them as workflow artifacts. It never signs, creates a release, publishes to npm or installs a service.
- `scripts/sign-release.js`: the owner's step. Verifies the CI binaries (recorded digest and `gh attestation verify`), signs them with the offline key through `assemble-release.js`, and writes the npm tarballs, offline bundles, `SHA256SUMS`, the public key and its fingerprint.
- `.github/workflows/smoke.yml`: installs the signed packages on eight clean runners (`dry-run` signs with a throwaway key made inside the run; `release` installs real release assets and checks the key fingerprint you pass in).
- `scripts/build-manylinux.sh`, `scripts/check-glibc.sh`, `scripts/validate-sbom.py`: the Linux build recipe, the glibc ceiling check and the CycloneDX 1.5 schema validation.

## Local build and signed assembly

Prerequisites for maintainers: pinned Rust/Cargo 1.95.0 with compiler/native build tools, Node 22+, and an externally managed Ed25519 key pair. **End users do not need Rust/compiler tools.** Never generate production private keys in the workspace or include them in packages. Public-key fingerprints must be authenticated independently of the artifact being verified.

From repository root, build the Linux development binary:

```sh
cargo build --release --locked -p omg-cli
```

The assembly output parent must already exist; output itself must not exist. Example using a maintainer-selected local output directory and externally stored keys:

```sh
node scripts/assemble-release.js \
  --binary target/release/omg \
  --target linux-x64-gnu \
  --version 0.1.0 \
  --public-key /trusted/key-location/omg-release-public.pem \
  --private-key /private/key-location/omg-release-private.pem \
  --out /existing/output-parent/omg-0.1.0
```

Private keys must match the explicitly supplied public root and have restrictive permissions on Unix. The source trust file contains **no trusted key** and cannot launch unsigned payloads. Assembly pins the supplied public key only in generated packages, reports its SHA-256 SPKI fingerprint, verifies the final artifact and refuses to overwrite existing output. Binary ELF64/PE/Mach-O architecture is checked against the target; ABI/OS minimum versions still require native tests.

Generated output:

```text
oh-my-gigabytes/                   # wrapper + exact-version optional native dependency
oh-my-gigabytes-linux-x64-gnu/     # example native package; artifact/omg + manifest + signature
offline-oh-my-gigabytes/           # same public package name, embedded verified native artifact
```

Other supported *assembly identifiers*: `linux-arm64-gnu`, `darwin-arm64`, `darwin-x64`, `win32-x64`. This is packaging recognition, not native support certification. Only GNU/glibc Linux packages are recognized; musl fails with a useful message. GNU toolchain binaries need a tested libc/runtime baseline; wrapper checks GNU presence, not minimum symbol compatibility.

## Manifest contract

Ed25519 signs `OMG-BINARY-MANIFEST-V1\0` followed by the **exact UTF-8 manifest bytes**. The signature is raw 64-byte `manifest.sig`. Schema 1 contains only:

- `schema`, application `version`, platform `target`, Rust `triple`.
- Fixed `file` (`omg` or `omg.exe`), exact `size`, SHA-256 `sha256`, pinned `keyId`.
- `capabilities`: Linux analysis and service `development-linux`; Windows analysis `development-windows` and service `disabled`; macOS analysis `development-macos`, cleanup `disabled` and service `disabled`. Permanent delete is `development-unqualified` on Linux and Windows full builds (`disabled` for the analysis-only variant and other OS). The binary refuses unknown filesystems, and on Windows everything except NTFS. OS Trash is not implemented and is not declared. The manifest field is still named `cleanup`; its value is the permanent-delete capability. The packages still never register or start a service; registration is the user's explicit `omg start`/`omg service install`.

The launcher rejects unsupported fields/schema/capabilities, unsigned/untrusted metadata, signature tampering, mismatched version/target, traversal file names, oversize/redirected files and changed or wrong-format binary payloads. It re-verifies **every launch** and spawns the absolute packaged binary with inherited stdio/argv, no shell or PATH replacement. Signals/exit codes are forwarded. No automatic network fallback.

This trust boundary assumes installed wrapper/binary directories remain user-controlled. It is not a defense against a fully compromised account replacing both the trust root and application or racing a pathname after verification. Native handle-bound execution/installer ACL hardening, artifact ancestor redirection and signing-key governance remain production gates. A signed manifest authenticates bytes and declared identity, not correctness, minimum OS compatibility or safety certification.

## Offline/manual verification

Given an already trusted public key and transferred artifact directory:

```sh
node scripts/verify-release.js \
  --artifact /offline/bundle/oh-my-gigabytes-linux-x64-gnu/artifact \
  --public-key /trusted/key-location/omg-release-public.pem \
  --target linux-x64-gnu --version 0.1.0
```

Transfer the verifier and trust material through a trusted channel along with artifacts; do not blindly trust a public key contained in a download. A verifier runs without network. After successful verification, the native `omg` binary can be installed into a trusted user-owned PATH location and run without Node/Rust. Exact copy/permissions commands depend on the platform; do not overwrite an unrelated executable or alter VHD/system data.

For offline npm use, pack the **self-contained offline wrapper** on a connected/build machine:

```sh
npm pack /offline/bundle/offline-oh-my-gigabytes --ignore-scripts
npm install --global --ignore-scripts --offline /transfer/oh-my-gigabytes-0.1.0.tgz
omg --version
omg doctor
```

The normal wrapper tarball with unresolved optional dependencies is not a complete offline bundle. Generated native/wrapper tarballs are private/unpublished and require distribution from a trusted maintainer. No production name-ownership/key fingerprint or public download URL exists yet.

`omg doctor` now avoids creating/migrating state and performs read-only version/schema/integrity diagnosis when an index exists. Read-only SQLite queries may use WAL shared-memory machinery; this does not claim the OS makes no metadata/SHM changes. Detailed diagnostic errors can still contain paths and need final redaction review.

## Upgrade and uninstall

Use a verified matching-version package, retain readable user state and allow application schema migration only through normal index opening. Local reinstall tests cover same-version package replacement, not all historical/future schema upgrades. Supported old schema tests and backups remain release gates.

```sh
npm uninstall --global oh-my-gigabytes
```

Uninstall removes the launcher/platform package; it **preserves the local index/history/preferences**. No service is installed by these packages. If you registered the service, run `omg service uninstall` first (npm lifecycle hooks are not relied on); `omg doctor` reports an orphaned registration whose binary no longer exists. Uninstall never cleans scanned user files or Trash. State removal is a separate explicit scoped user decision.

## Releasing the beta

Everything below runs on a commit that has passed `Checks`. Steps marked **owner** need the owner's key, account or explicit go; nothing is automatic.

1. **Build.** Run the `Release build` workflow on the commit (`gh workflow run release.yml --ref main`, or push a `v*` tag, which is an owner action). It uploads `omg-<target>` artifacts and an SBOM.
2. **Download.** `gh run download RUN_ID --dir ci-artifacts`.
3. **Sign (owner).** With the production key kept outside the repository and `gh` 2.49 or newer:

   ```sh
   node scripts/sign-release.js --artifacts ci-artifacts --version 0.1.0-beta.1 \
     --public-key /safe/release-public.pem --private-key /safe/release-private.pem \
     --out signed --repo akramcodez/oh-my-gigabytes
   ```

   Add `--publishable` only for the build you intend to put on npm; without it the packages stay `private` and npm refuses to publish them.
4. **Check locally.** Install `signed/offline/oh-my-gigabytes-offline-<target>-<version>.tgz` on your machines (WSL and PowerShell) and run `omg --version`, `omg doctor`, `omg scan`. `node scripts/verify-release.js` verifies an artifact directory offline.
5. **Draft release (owner).** Create a draft GitHub Release and upload `signed/tarballs/*`, `signed/offline/*`, `SHA256SUMS`, `release-public.pem`, `KEY-FINGERPRINT.txt` and the SBOM. Publish the key fingerprint somewhere independent of the release (the README, `SECURITY.md`, a post).
6. **Smoke.** Run `Install smoke test` with `source=release`, the release tag and the trusted fingerprint (and `previous_tag` for an upgrade check).
7. **npm (owner).** `npm publish --tag beta` each tarball with the platform packages first and `oh-my-gigabytes-<version>.tgz` last, so a plain `npm install oh-my-gigabytes` never selects the beta. Names `oh-my-gigabytes` and `oh-my-gigabytes-<target>` were unclaimed on 2026-10-18.
8. **Publish the release**, and record the run ids in [beta-release-progress.md](beta-release-progress.md).

A user verifies a manual download with `sha256sum -c SHA256SUMS` in the download folder, compares `KEY-FINGERPRINT.txt` with the fingerprint published elsewhere, and installs with `npm install --global --ignore-scripts --offline <offline tarball>`. On macOS a binary downloaded in a browser carries a quarantine mark and is not notarized; the npm path avoids that, and `xattr -d com.apple.quarantine` clears it for a manual copy.

## Release controls still required

- Confirm npm name/platform-package ownership and repository/publisher identities; consider trusted publishing for provenance on npm (a local publish has none). Never ship the dry-run key.
- CI actions are pinned by commit SHA and updated through Dependabot; keep reviewing those pull requests. Hosted runners now test Linux x64/arm64, Windows x64/arm64 and macOS arm64/Intel; `cargo deny` and the SBOM schema check run in CI.
- Authenticate release key fingerprints; implement rotation/revocation and offline trust instructions with actual identities.
- Sign final bytes after any code signing/notarization, attest actual binary and npm builds, inspect package contents/notices and release matrix.
- Certify native metadata/cloud/Trash/state permissions, recovery and every advertised capability. macOS/Windows stub builds are not cross-platform cleaner support.

No production release or automatic publishing workflow is enabled by this milestone.


## Local signed Linux build (0.1.0, analysis-only, not published)

Status: a local, signed test artifact. Nothing is uploaded or published. It is not qualified and it does not run on glibc older than 2.39.

| Item | Value |
| --- | --- |
| Location | `dist/oh-my-gigabytes-0.1.0-linux-x64-gnu/artifact/omg` (git-ignored by `/dist/`) |
| Files | `artifact/omg`, `artifact/manifest.json`, `artifact/manifest.sig`, `SHA256SUMS` |
| Variant | `analysis-only` (built with `--no-default-features`; permanent delete is compiled off) |
| Target | `linux-x64-gnu` (x86_64-unknown-linux-gnu) |
| Size | 6050792 bytes |
| SHA-256 | `5fdc6e4c3c4f64b5b0df0fff19f68fb0799d03d0866065add07ed4859462cd4c` |
| Signing key ID | `22dd52bb7e7e006092f50d05abdb38e8019402c4d45b6f3a53c6cc96c23dff25` |
| Claimed capabilities | analysis `development-linux`, cleanup `disabled`, service `development-linux` |
| Minimum glibc | 2.39 (the build host has 2.43) |

### How it was produced

```sh
# 1. Build the analysis-only binary (permanent delete compiled off).
cargo build --release -p omg-cli --no-default-features --locked
cp target/release/omg ~/omg-builds/omg-analysis-only

# 2. Sign it. Keys live outside the repo. The private key must be 0600.
node scripts/assemble-release.js \
  --binary ~/omg-builds/omg-analysis-only --target linux-x64-gnu --version 0.1.0 \
  --variant analysis-only \
  --public-key ~/omg-release-keys/release-public.pem \
  --private-key ~/omg-release-keys/release-private.pem \
  --out /tmp/omg-release-0.1.0-analysis

# 3. Copy the signed artifact into the repo (git-ignored) and verify the copy.
mkdir -p dist/oh-my-gigabytes-0.1.0-linux-x64-gnu/artifact
cp /tmp/omg-release-0.1.0-analysis/oh-my-gigabytes-linux-x64-gnu/artifact/* dist/oh-my-gigabytes-0.1.0-linux-x64-gnu/artifact/
sha256sum dist/oh-my-gigabytes-0.1.0-linux-x64-gnu/artifact/omg > dist/oh-my-gigabytes-0.1.0-linux-x64-gnu/SHA256SUMS
node scripts/verify-release.js \
  --artifact dist/oh-my-gigabytes-0.1.0-linux-x64-gnu/artifact \
  --public-key ~/omg-release-keys/release-public.pem --target linux-x64-gnu --version 0.1.0
```

### Checks performed

- `assemble-release.js` refuses a `full` claim for the analysis-only binary and refuses to write anything. It runs the binary's own `doctor` to confirm the claim.
- `verify-release.js` passes on the copy in `dist/`.

### Limits

- The test key in `~/omg-release-keys` is for dry runs. Do not publish with it. A real release needs a new key, held by the owner.
- Not built with an older glibc. Use a container baseline before any public upload (see the checklist).
- The copy in `dist/` is local only. Publishing requires explicit authorization.
