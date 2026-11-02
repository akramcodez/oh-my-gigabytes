# oh-my-gigabytes

OMG (Oh My Gigabytes) is a local-first storage analyzer for developers, with a thin npm launcher. This is a development-preview beta (`0.1.0-beta.1`). Scanning reads metadata only and never uploads anything. The Linux and Windows binaries can also permanently delete files you pick, only after you confirm an exact list, and that feature is unqualified (see the repository README, "Deleting files"). The macOS binary cannot delete.

The launcher verifies the signed target/version/capability manifest and native SHA-256 digest **on every launch**, then passes arguments/stdin/stdout/stderr to the packaged `omg` binary without a shell. It does not search PATH for a replacement executable. There is no postinstall script, network downloader, telemetry, automatic service registration or end-user Rust compilation. Missing optional dependencies produce a useful error and signed offline-bundle guidance.

Node.js 22+ is required for the npm path. Manual binaries do not need Node or Rust to run.

Platforms (development preview; none is certified): Linux x86_64 and ARM64 with glibc 2.28+ and a 5.6+ kernel, Windows 11 x64 only (NTFS and ReFS), and macOS 15+ on Apple silicon only (APFS, HFS+, exFAT, FAT). The background scan service exists on Linux only. Permanent delete is in the Linux and Windows builds (`omg doctor` shows `permanent delete: development-unqualified`) and absent from the macOS build. Windows and macOS have been tested on hosted runners (Windows also on a Windows 11 PC); see the platform evidence pages in the source repository.

See `docs/release.md` in the source repository for local assembly, verification, offline installation, upgrades and uninstallation, and the repository README for the supported-platform table. A package is trustworthy only if its signing key fingerprint matches one published separately by the maintainer.
