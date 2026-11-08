# Development distribution, authenticity and installation

Status: **not published yet**; see [Current state](#current-state-2026-11-08). **Update (2026-10-24, owner decision):** the first release is a development-preview beta (`0.1.0-beta.1`) for Linux, Windows and macOS whose binaries include permanent delete, declared `development-unqualified` (macOS since the owner's decision of 2026-11-07, APFS only). See [Releasing the beta](#releasing-the-beta) and [Not passed yet](#not-passed-yet). The analysis-only variant (`--no-default-features`, delete compiled out) is still built and tested in CI. OS Trash is not implemented.

## What exists

- `packages/oh-my-gigabytes`: Node 22+ launcher with `omg` bin, no runtime dependencies/download/install scripts.
- `scripts/assemble-release.js`: builds exact-version platform package, wrapper with optional dependency, and self-contained offline wrapper from an explicitly supplied binary and maintainer-controlled Ed25519 key pair.
- `scripts/verify-release.js`: offline verification using an independently trusted public key, expected target and version.
- `scripts/sbom.js`: CycloneDX 1.5 dependency inventory from locked Cargo metadata; validate against the CycloneDX schema and review licenses before production release.
- `.github/workflows/release.yml`: builds the binaries for all four targets (Linux x64 and arm64 in a `manylinux_2_28` container and Windows x64 with a static C runtime, all with the default features and so with permanent delete; macOS arm64 with `--no-default-features`; Intel Macs are not released), checks that each reports the cleanup capability its target is meant to have, records that value in a `CLEANUP` file next to the binary, attests their provenance and uploads them as workflow artifacts. It never signs, creates a release, publishes to npm or installs a service.
- `scripts/sign-release.js`: the owner's step. Verifies the CI binaries (recorded digest and `gh attestation verify`), signs them with the offline key through `assemble-release.js` (the signed cleanup capability comes from each build's `CLEANUP` file, so a manifest cannot claim less or more than its binary reported; a missing or unknown value stops the signing), and writes the npm tarballs, offline bundles, `SHA256SUMS`, the public key and its fingerprint.
- `.github/workflows/smoke.yml`: installs the signed packages on seven clean runners (`dry-run` signs with a throwaway key made inside the run; `release` installs real release assets and checks the key fingerprint you pass in).
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

Other supported *assembly identifiers*: `linux-arm64-gnu`, `darwin-arm64`, `win32-x64`. An Intel Mac (`darwin-x64`) is not a target and the launcher reports it as an unsupported platform. This is packaging recognition, not native support certification. Only GNU/glibc Linux packages are recognized; musl fails with a useful message. GNU toolchain binaries need a tested libc/runtime baseline; wrapper checks GNU presence, not minimum symbol compatibility.

## Manifest contract

Ed25519 signs `OMG-BINARY-MANIFEST-V1\0` followed by the **exact UTF-8 manifest bytes**. The signature is raw 64-byte `manifest.sig`. Schema 1 contains only:

- `schema`, application `version`, platform `target`, Rust `triple`.
- Fixed `file` (`omg` or `omg.exe`), exact `size`, SHA-256 `sha256`, pinned `keyId`.
- `capabilities`: Linux analysis and service `development-linux`; Windows analysis `development-windows` and service `disabled`; macOS analysis `development-macos`, cleanup `disabled` and service `disabled`. Permanent delete is `development-unqualified` on Linux and Windows full builds, which is what the beta ships there (`disabled` for the analysis-only variant and on macOS). The binary refuses unknown filesystems, and on Windows everything except NTFS. OS Trash is not implemented and is not declared. The manifest field is still named `cleanup`; its value is the permanent-delete capability. The packages still never register or start a service; registration is the user's explicit `omg start`/`omg service install`.

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
4. **Check locally.** Install `signed/offline/oh-my-gigabytes-offline-<target>-<version>.tgz` on your machines (WSL and PowerShell) and run `omg --version`, `omg doctor` (all must print `permanent delete: development-unqualified`), `omg scan`. Because these builds can delete, delete one throwaway file under a folder you created for the purpose (use a separate `HOME`/`USERPROFILE` if in doubt) before publishing, and check that Cancel leaves it alone. `node scripts/verify-release.js` verifies an artifact directory offline.
5. **Draft release (owner).** Create a draft GitHub Release and upload `signed/tarballs/*`, `signed/offline/*`, `SHA256SUMS`, `release-public.pem`, `KEY-FINGERPRINT.txt` and the SBOM. Publish the key fingerprint somewhere independent of the release (the README, `SECURITY.md`, a post).
6. **Smoke.** Run `Install smoke test` with `source=release`, the release tag and the trusted fingerprint (and `previous_tag` for an upgrade check).
7. **npm (owner).** `npm publish --tag beta` each tarball with the platform packages first and `oh-my-gigabytes-<version>.tgz` last, so a plain `npm install oh-my-gigabytes` never selects the beta. Names `oh-my-gigabytes` and `oh-my-gigabytes-<target>` were unclaimed on 2026-10-18.
8. **Publish the release**, and record the run ids under [Not passed yet](#not-passed-yet).

A user verifies a manual download with `sha256sum -c SHA256SUMS` in the download folder, compares `KEY-FINGERPRINT.txt` with the fingerprint published elsewhere, and installs with `npm install --global --ignore-scripts --offline <offline tarball>`. On macOS a binary downloaded in a browser carries a quarantine mark and is not notarized; the npm path avoids that, and `xattr -d com.apple.quarantine` clears it for a manual copy.

## Current state (2026-11-08)

Candidate: `main` at `c5b03a6`. `Checks` passed on `ce0528f`; `c5b03a6` changes only `SECURITY.md`.

Done:
- Production Ed25519 key created by the owner (held outside the repository, never in CI). Fingerprint (`KEY-FINGERPRINT.txt`): `01deacfe588025e045419b57c4f39abd9f151294c8310e0a0dd28a18c9d9adc0`. Publish it somewhere independent of the release before announcing.
- Security contact in `SECURITY.md`; GitHub private vulnerability reporting enabled.
- npm login as `skakram`. The names `oh-my-gigabytes` and `oh-my-gigabytes-<target>` were still unclaimed on 2026-11-08.
- `Release build` run 42444990692 on `c5b03a6`: all four targets and the SBOM; every `CLEANUP` is `development-unqualified`. Signed by the owner with `--publishable` into a local `signed/` folder (outside the repository): 9 files, all matching `SHA256SUMS`, every package `private: false`.
- macOS, signed package installed with npm into a prefix with a space: `--version`, `doctor` (`development-unqualified`), no quarantine mark, Cancel kept the file, "Yes, delete permanently" removed only that file (a `.pem` and a neighbour stayed), one appended byte refused with "Binary integrity verification failed".

Next, in order:
1. Owner: the same install and throwaway delete on a Windows 11 PC and a Linux machine (step 4 of [Releasing the beta](#releasing-the-beta)). In progress.
2. Owner: draft GitHub Release with `signed/tarballs/*`, `signed/offline/*`, `SHA256SUMS`, `release-public.pem`, `KEY-FINGERPRINT.txt` and the SBOM.
3. `Install smoke test` with `source=release`, the tag and the fingerprint (never run in this mode).
4. Owner: `npm publish --tag beta`, platform packages first and the wrapper last (npm requires two-factor authentication), then publish the GitHub Release.

## Not passed yet

- Linux ARM64 and Windows on Arm have only hosted-runner evidence; macOS has hosted runners plus one Apple silicon Mac, without Full Disk Access, iCloud placeholder or real external-drive checks (`mac-final-check.md` steps 5 to 8b).
- Permanent delete is not qualified on any platform: no recorded native evidence per filesystem, the XFS, Btrfs and old-kernel runs are missing, and no CI-built binary has deleted anything on a real Windows 11 or Linux machine yet. It ships as `development-unqualified` on the owner's decision, with that label in the README, `omg doctor` and the signed manifest.
- Kernel floor, XFS and Btrfs native runs, service qualification, Windows hard-link totals, ConPTY and virtual-disk matrices, benchmarks, usability review.
