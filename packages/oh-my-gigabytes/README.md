# oh-my-gigabytes

OMG is a Rust storage-intelligence development preview with a thin npm launcher.

This source package is private and has no release trust key or native binary. It fails closed until a maintainer assembles a signed artifact and pins its release public key in the generated wrapper. There is no postinstall script, network downloader, telemetry, automatic service registration or end-user Rust compilation.

The launcher verifies the signed target/version/capability manifest and native SHA-256 digest **on every launch**, then passes arguments/stdin/stdout/stderr to the packaged `omg` binary without a shell. It does not search PATH for a replacement executable. Missing optional dependencies produce a useful error and signed offline-bundle guidance.

Node.js 22+ is required for the npm path. Manual binaries do not need Node or Rust to run. Current analysis is limited to supported Linux local metadata profiles; macOS/Windows native analysis, cleanup and services are disabled pending certification. Linux GNU builds must also satisfy the artifact's tested runtime ABI baseline.

See `docs/release.md` in the source repository for local assembly, verification, offline installation, upgrades and uninstallation. Source package signing roots are not provisioned; nothing here is a published or certified cross-platform release.
