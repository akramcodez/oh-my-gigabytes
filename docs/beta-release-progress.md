# Analysis-only beta: progress

Target: `0.1.0-beta.1`, analysis only (permanent delete compiled out), Linux, Windows and macOS. Status as of 2026-10-19: **not released.** Nothing is tagged, uploaded or published, and no production signing key exists. Evidence lives in the platform pages: [linux-qualification.md](linux-qualification.md), [windows-evidence.md](windows-evidence.md), [macos-evidence.md](macos-evidence.md), [release.md](release.md).

Scope decisions (owner, 2026-10-18): analysis-only first; native macOS analysis adapter tested on GitHub-hosted runners; GitHub Releases plus npm, Ed25519 signing only (no Authenticode, no Apple notarization).

## Work packages

| WP | What | Status | Evidence / what remains |
| --- | --- | --- | --- |
| R00 | Owner environment | Partly | Windows PC: Rust 1.95.0 GNU and MSVC toolchains and MinGW present, MSVC linker absent, C: has 7.4 GB free (Build Tools need about 7 GB), so local Windows builds use the GNU toolchain and CI builds MSVC. WSL: Docker 29.1.3 and systemd work; `xfsprogs`/`btrfs-progs` not installed; `sudo` needs a password. The WSL disk file is 173.5 GB on D: with 0.2 GB free on the host: do not grow it. |
| R01 | CI matrix, pinned actions, Dependabot | Done | `Checks` runs on ubuntu-22.04, ubuntu-24.04, ubuntu-24.04-arm, windows-2022, windows-latest, windows-11-arm, macos-15, macos-15-intel, macos-26, plus a licenses/advisories/SBOM job. Run 39111223427 on commit `ad22c81`: all green. |
| R02 | Tests portable across platforms | Partly | Scanner, rollup, policy, planning, four CLI tests and the new migration tests run on Linux and macOS (`cfg(unix)`), Windows has `tests/windows.rs`. `service.rs`, `cleanup.rs` and the Btrfs tests stay Linux-only by design. |
| R03 | macOS probe | Done | [macos-evidence.md](macos-evidence.md). Runs 39106110919 and 39106517191. |
| R04 | macOS adapter | Done | `platform::macos`, passing on macos-15, macos-15-intel and macos-26 (run 39111223427). |
| R05 | macOS policy and catalog | Partly | Mandatory exclusions and system paths done. Protected paths (`Library/Keychains`, Mail, Messages, Safari, Group Containers) and macOS cache rules in the catalog are not done. |
| R06 | macOS tests | Done | `tests/macos.rs` mounts APFS, case-sensitive APFS, HFS+, case-sensitive HFS+, exFAT and FAT32 images, scans each, and checks the mount boundary; PTY smoke test and quarantine test also run. TCC, iCloud and Gatekeeper dialogs are not verifiable on hosted runners. |
| R07 | Linux errors and XFS | Partly | Clear "Linux 5.6+" error and XFS scanning allowed (unit-tested); `scripts/fs-sandbox.sh ext4\|xfs\|btrfs` written. Not run: XFS and Btrfs sandboxes (need `sudo` and the mkfs tools), seccomp `openat2` simulation. |
| R08 | Linux build baseline and distro matrix | Done | Release builds in `manylinux_2_28` (x86_64 and aarch64); `scripts/check-glibc.sh` asserts glibc 2.28 in CI. Container matrix on 9 distros passed locally. The kernel floor (5.6) is not tested on an older kernel. |
| R09 | Service qualification | Not started | Fake `notify-send` test, second-user IPC test, idle cost, journald policy. |
| R10 | Migrations and recovery | Done | `tests/migrations.rs`: schemas 1 to 11 upgrade and keep data; newer and damaged indexes are refused and left untouched. Disk-full injection is not done. |
| R11 | Windows state-directory ACL | Done | `platform::windows_state`; passes natively on the owner's PC and on windows-2022, windows-latest and windows-11-arm. |
| R12 | Windows hard-link totals | Not started | Folder totals still count each name of a hard-linked file. |
| R13 | Windows CI, ConPTY, virtual disks | Partly | Windows CI legs green. ConPTY smoke test, `diskpart` virtual-disk script, ReFS/FAT32/exFAT runs, `C:\` and OneDrive manual checks not done. |
| R14 | win32-x64 packaging | Done | Static CRT via `.cargo/config.toml`; the released binary imports only system DLLs. |
| R15 | Measurements | Not started | |
| R16 | Supply chain | Partly | `deny.toml` (advisories, licenses, sources) and CycloneDX 1.5 schema validation run in CI. The `unsafe` audit is not done. |
| R17 | README and support table | Not started | |
| R18 | Release workflow, capabilities, version | Done | `release.yml` builds all five targets with attestations (run 39111562168). macOS declares `development-macos`. Releases default to analysis-only. Version is `0.1.0-beta.1`. |
| R19 | Owner signing procedure | Done | `scripts/sign-release.js`, tested on the CI binaries with a throwaway key. Needs `gh` 2.49 or newer for `gh attestation verify`. |
| R20 | Clean-machine smoke | Done for dry-run | `smoke.yml` dry-run (run 39196125718) passed on eight machines: build, attest-verify, sign with a throwaway key, install, run, scan, reinstall, uninstall with state kept, offline bundle. The `release` mode (real assets, trusted key fingerprint, upgrade from an earlier release) has not been run. |
| R21 | Owner items | Not started | Production key, `SECURITY.md` contact, GitHub private vulnerability reporting, npm names (unclaimed on 2026-10-18), npm publishing method. |
| R22 | Release candidate | Not started | Needs every gate above and an explicit go from the owner. |

## Not passed yet (blocks calling this a release)

- No production key or publisher identity; no real release has been signed.
- Linux ARM64, macOS and Windows have only hosted-runner evidence. macOS stays `development-macos`.
- Kernel floor, XFS and Btrfs native runs, service qualification, Windows hard-link totals, ConPTY and virtual-disk matrices, benchmarks, usability review (V01).
