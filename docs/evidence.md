# Evidence

What was verified for behaviour that is not specific to one platform. Platform pages: [Linux](linux-qualification.md), [macOS](macos-evidence.md), [Windows](windows-evidence.md). The earlier phase, planning and research pages were removed to keep the repository small; they are in Git history at `970809b`.

## Old toolchain versions, logs and temp files, scripted clean, what grew, daily scans (2026-11-14)

Owner request (1.0 list items 5 to 12, without a release). Implemented:

- `toolchain_old_versions` (category `old_tool_versions`, suggested): direct folders of rustup `toolchains`, nvm `versions/node` and pyenv `versions`, grouped by line (Node major, Python major.minor, Rust release or nightly per target triple). The newest of a line is kept and must be a day old; an older one must be unchanged for 30 days. Channel toolchains, two-part versions and pyenv versions with `envs` are never listed.
- `home_logs_and_temp` (category `logs_and_temp`, suggested, macOS and Windows): direct items of `~/Library/Logs`, `AppData/Local/Temp` and `AppData/Local/CrashDumps`, 1 MB or more, modified at least 7 days ago. Removal goes through the same `verdict`, `prepare_delete` and `delete_permanently`.
- `omg clean --category C [--older-than N[d]] [--confirm FP]`: `select_for_clean` lists unprotected findings of the categories with a fingerprint over their finding ids; `clean_selection` refuses a different fingerprint and then marks, plans and deletes through the single deletion path. Whole repositories and session transcripts stay one at a time and end up denied.
- `freed_total_bytes` in `omg status` and "freed so far" in the interface: the sum of completed deletions and Docker removals in `operations`.
- `omg grew` and a "Grew" line in Overview: migration 013 adds `size_snapshots`; each complete scan stores folders of 1 MB or more (top 2000) and keeps two per root.
- `omg schedule enable|disable|status` (`scheduled.rs`): macOS launchd agent or Windows Task Scheduler task running only `scan --accept-gaps --no-suggest`; the executable must be a regular file not writable by others; only a file carrying omg's marker is replaced or removed. Linux keeps `omg start`.
- `OUTPUT_SCHEMA` 1 in `status` and `doctor`, with the promise in [compatibility.md](compatibility.md); a reviewer guide in [review.md](review.md).

Verified on the owner's Apple silicon Mac (macOS 27.2, Rust 1.95.0):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings                       # also --no-default-features
TMPDIR=$(cd "$TMPDIR" && pwd -P) OMG_REQUIRE_DISK_IMAGES=1 cargo test --workspace --locked   # also --no-default-features: all pass
python3 -m unittest discover -s tests -p 'test_*.py'                                  # 8 run, 3 skipped (Linux only)
node --test packages/oh-my-gigabytes/test/release.test.js                            # 21 pass
```

New tests: `toolchain_version_folders_group_by_line_and_skip_channels` (catalog), `replaced_toolchain_versions_are_suggested_and_channels_and_envs_are_kept`, `old_home_logs_are_suggested_item_by_item` and `scripted_clean_selects_by_category_and_age_and_deletes_only_a_confirmed_selection` (`tests/cleanup.rs`), `grew_names_the_innermost_folder_that_grew_between_the_last_two_scans` (`tests/scanner.rs`), `scheduled` unit tests and `tests/scheduled.rs` (fake `launchctl`, macOS), and the output contract in the CLI status test. A smoke run of `scan`, `grew`, `status`, `clean`, `schedule status` and `doctor` against a private state directory behaved as described.

Not passed yet: the launchd job has not been loaded by the real `launchctl`, and the Task Scheduler path has run only as compiled code on CI, not against a real task. Logs and temp files on Windows rely on CI. The reviewer guide asks for an outside review of `cleanup.rs` and `docker.rs`, which has not happened.

## Xcode, ML models, more tool caches and editor extensions (2026-11-13)

Owner request after public beta feedback. New discoveries in `catalog::DISCOVERIES`, each a directory root resolved by `catalog::discovery_root` and then `tool_root_allowed` (so an environment override never takes in the home folder or a personal folder), and treated as a cache root at deletion:

- Every platform: Bun (`BUN_INSTALL_CACHE_DIR`, else `$BUN_INSTALL` or `~/.bun`, `/install/cache`), Maven `~/.m2/repository`, Puppeteer (`PUPPETEER_CACHE_DIR` or `~/.cache/puppeteer`), Hugging Face (`HF_HUB_CACHE`, else `$HF_HOME` or `~/.cache/huggingface`, `/hub`), Ollama (`OLLAMA_MODELS` or `~/.ollama/models`), LM Studio `~/.lmstudio/models`, and the overrides `DENO_DIR`, `PLAYWRIGHT_BROWSERS_PATH`, `COMPOSER_CACHE_DIR`.
- macOS: `~/Library/Developer/Xcode/DerivedData`, `~/Library/Developer/Xcode/iOS DeviceSupport`, `~/Library/Developer/CoreSimulator/Caches`, `~/Library/Caches/{deno,CocoaPods,ms-playwright,JetBrains}`. Linux: the XDG cache `{deno,composer,ms-playwright,JetBrains}`. Windows: `%LOCALAPPDATA%\{deno,Composer,ms-playwright}`; JetBrains is left out there because Toolbox installs IDEs in that folder.
- `editor_old_extensions`: in `~/.vscode/extensions` and `~/.cursor/extensions`, folders named `publisher.name-x.y.z[-platform]` are grouped by lowercased id and platform suffix; all but the numerically newest are suggested once the newest folder's modification time is more than a day old (a window opened before an update may still use the old one). Deletion stays `Kind::Plain`.
- Hugging Face, Ollama and LM Studio use the new category `ml_models`, which `Index::suggest` does not add: last use of a model cannot be told from metadata, and Ollama shares blobs between models (the rule points to `ollama rm`). Maven warns that `mvn install` artifacts must be rebuilt; Xcode and JetBrains rules say to quit the app first.
- The interface's finding details show "Last built" (build output) or "Last changed" from the subtree's newest modification time; `omg findings` already reported it in `subtree.newest`.

Left out on purpose: Xcode Archives (they hold app bundles, which are protected, and dSYMs that cannot be recreated), Simulator runtimes (outside the home folder) and Simulator devices (they hold app data; `xcrun simctl delete unavailable` is the right tool).

Verification on the development Mac (macOS 27.2, Rust 1.95.0): unit tests `extension_folder_names_split_into_id_version_and_platform` and `new_tool_folders_use_each_tools_documented_default`; `tests/cleanup.rs` `older_editor_extension_versions_are_suggested_once_the_newest_has_settled` (numeric order, per platform suffix, case-insensitive ids, a just-installed newest keeps the older one, the suggested folder deletes through a confirmed plan) and `large_tool_caches_and_models_are_found_and_only_caches_are_suggested`. Full suite results are in the pull request.

Not passed yet: no real Xcode, Ollama, LM Studio or JetBrains folders were scanned; Linux and Windows locations are covered only by CI and by the documented defaults.

## Unused Docker data (2026-11-12)

Owner request. Docker's disk is never deleted (one file holds every image, container and volume); instead `omg docker` and the interface (`m` › Clean unused Docker data) ask the engine itself.

- Reached only locally: `DOCKER_HOST` if it is `unix://` or `npipe://` (anything else is refused), otherwise the known per-user sockets (Docker Desktop, Colima, OrbStack, Rancher Desktop, `/var/run/docker.sock`, rootless Docker and Podman) or `\\.\pipe\docker_engine` on Windows. A socket must be owned by the current user or root.
- Read: `GET /system/df`. Suggested by default: dangling images (no tag, no container) created 7 or more days ago and build cache not in use and unused for 30 or more days. Opt-in: tagged images no container uses and stopped containers, created 30 or more days ago. Never: volumes, running or paused containers, anything in use, or anything without a date.
- Removal: a fingerprinted plan the user confirms (the interface dialog defaults to Cancel; the CLI needs `--confirm <fingerprint>`). `Engine::remove` refuses if the plan is older than 15 minutes or if Docker no longer reports exactly the reviewed items, re-reads Docker before each item, and removes with `DELETE /images/<id>?force=false`, `DELETE /containers/<id>?v=false&force=false` or `POST /build/prune` filtered by id and `inuse=false`. `check_request` refuses every other request. Each removal is recorded in `operations` (kind `docker`) before it starts.

Verification on the development Mac (macOS 27.2, Rust 1.95.0): unit tests (request allowlist, remote hosts refused, HTTP chunked and sized bodies, RFC 3339 dates, classification and ages) and `crates/omg-core/tests/docker.rs` against a fake engine on a Unix socket (only planned items removed and without force; nothing removed when Docker changed or the confirmation is wrong; an item that comes into use mid-removal is skipped; opt-in kinds never include running containers). `cargo test --workspace` in both builds and clippy `-D warnings` pass.

Not passed yet: a real Docker engine on any platform (no Docker was running on the development Mac), the Windows named pipe (compiled in CI only), and Podman's compatibility API.

## Unused applications and toolchains (2026-11-08)

Requested by the owner after a Windows scan where `AppData` held 45 GiB of 62 GiB and `.rustup` 6 GiB. Two discoveries, on Linux, macOS and Windows, feed `Index::suggest` (categories `unused_apps`, `unused_toolchains`); listing authorizes nothing and every item still goes through a confirmed plan.

- `unused_app_data`: a direct subfolder of `%USERPROFILE%\AppData\{Local,LocalLow,Roaming}` (plus `LOCALAPPDATA`/`APPDATA`), `~/Library/Application Support`, or `~/.local/share`, `~/.config`, `~/.cache` (plus the XDG overrides); 50 MB or more; newest modification time in the subtree older than 150 days; no unreadable part; `dir_totals.protected = 0`, so a folder with a database, key or other protected item is never suggested (docs/safety.md, "Databases: deny parent removal that contains them"); not hidden, not `com.apple.*`, not a system/desktop/VM name (`Microsoft`, `Packages`, `Programs`, `Temp`, `Docker`, `wsl`, `Trash`, `keyrings`, `flatpak`, `containers`, `omg`, ...); and not overlapping a known cache, a toolchain or the AI tool's folders. At deletion it stays `Kind::Plain` (every descendant checked) and carries a warning that the app's settings and sign-ins go with it.
- `unused_toolchain`: `.rustup` (with `.cargo`), nvm (with `.npm`), pyenv, SDKMAN! (with `.gradle`, `.m2`), the Android SDK (with `.gradle`, `.android`), at the default location and any absolute override; 50 MB or more; it and its related folders unchanged for 150 days; and no folder named `target`, `node_modules`, `__pycache__`/`.venv`/`venv` or `build`/`.gradle` anywhere in the scan changed in that time. Toolchains are cache roots at deletion.

Limits: "unchanged" is the newest modification time, not last use. An app that only reads its folder looks unused; a build outside the scanned folder is not seen. Both rules say so in their warnings.

Environment: WSL2 (Linux 5.15.167.4-microsoft-standard-WSL2), ext4, Rust 1.95.0. Verification: `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --locked -- -D warnings` in both builds; `cargo test --workspace --locked` and `--no-default-features`, all passed; `python3 -m unittest discover -s tests -p 'test_*.py'` (8 run, OK, 3 skipped); `node --test packages/oh-my-gigabytes/test/release.test.js` (21 passed). New tests: `apps_untouched_for_five_months_are_suggested_with_a_warning_and_can_be_deleted` and `a_toolchain_unused_for_five_months_is_suggested_unless_something_used_it` in `tests/cleanup.rs` (recent app, database inside, small, `Trash`, hidden, recent build, recent `.cargo`, recent toolchain are all left out; the suggested folder is deleted through a confirmed plan), and `appdata_and_toolchains_unused_for_five_months_are_suggested_and_deletable` in `tests/windows.rs`. A scan of the WSL home (all of it younger than 3 months) produced no such suggestion, as expected.

Windows, natively on the owner's laptop (NTFS, Rust 1.95.0 `x86_64-pc-windows-gnu` with MinGW, `TMP` outside any repository): `cargo test -p omg-core --test windows --locked`, 17 passed, including the new test. A scan of the real profile (`omg --state-dir <private> scan --root "C:\Users\<owner>" --no-suggest`: 1,082,642 entries, 0 errors, 28.5 s debug build) then `omg findings` listed 9 `unused_apps` folders, 2.13 GiB in all, the largest 0.65 GiB (`AppData\Roaming\discord`). `.rustup` (6 GiB) was correctly not listed: toolchains in it had been used that day. Nothing was deleted; the copy, build output and state were removed afterwards.

Not passed yet: the macOS leg of `tests/cleanup.rs` was not run locally (no Mac here); it runs in CI. The MSVC Windows build was not run locally (no C++ build tools on the laptop); CI builds with MSVC.

## Index size (2026-11-07)

The index stored every path twice and a parent copy twice, and its file never shrank: deleted scans left free pages, so after one rescan it stayed about twice the size of its data. Changes:

- Schema 12 keys `entries` by `(scan_id, parent, name)` in a `WITHOUT ROWID` table, with `path = parent||name`. The `entry_identity` and `entry_parent` indexes and the separate primary-key index are gone. Migration 012 copies only the published scan and then runs one `VACUUM`.
- New databases use `auto_vacuum=INCREMENTAL`; `Index::prune_stale_locked` hands free pages back in the same bounded slices as pruning, so it runs after every scan, at interface launch and in `omg prune`. `journal_size_limit` caps a kept write-ahead log at 32 MiB.
- The latest unpublished scan keeps its record and issues but no longer its entries and folder totals, which nothing read.
- `omg status` reports `index_bytes`. The release profile uses fat LTO, one codegen unit and strip.

Environment: Apple M5 Pro Mac, macOS 27.2, Rust 1.95.0, analysis-only release build. Old code is `e6d9d1b`, built from a clean export.

| Measurement | Before | After |
| --- | --- | --- |
| `/Applications`, 147,874 entries, after three scans | 150.0 MB | 30.4 MB |
| Synthetic tree, about 612,000 entries, first scan | 208 MB | 87 MB |
| Same tree, after three scans | 421 MB | 88 MB |
| Same tree, scan time (first, then rescans) | 2.77 s, about 3.45 s | 2.31 s, about 3.25 s |
| Existing 157.3 MB index, migrated on first open | n/a | 27.4 MB in 0.82 s |
| Release binary, macOS arm64 (gzip) | 4.85 MB (2.20 MB) | 3.76 MB (1.88 MB) |

Entry count and unique totals in `omg status` were identical between the old and new builds on the synthetic tree; `tree` output was identical between a fresh and a migrated index on `/Applications`.

Verification on that Mac: `cargo fmt --check`; clippy `-D warnings` in both builds; `cargo test --workspace --locked` and `--no-default-features` (analysis-only 77 passed, 0 failed, 1 ignored), with `OMG_REQUIRE_DISK_IMAGES=1`; PTY tests 7 run, OK, 5 skipped; `release.test.js` 18 passed, 3 skipped. New tests: exact non-UTF-8 path bytes through the migration, siblings such as `a.b` staying outside a subtree, the file shrinking after pruning, and the latest unpublished scan keeping only its record.

Not passed yet: these query changes in the delete path have run on macOS only (`crates/omg-core/tests/cleanup.rs`, see [macos-evidence.md](macos-evidence.md), "Permanent delete"), not on Linux or Windows; `tests/service.rs` and `tests/windows.rs` do not run on a Mac. A rescan still holds two generations until it publishes, so the peak during a rescan is about twice the final size (keeping the previous valid index requires this; the peak was not measured). Hard-link checks on Windows now sort instead of reading an index, and that cost has not been measured on a Windows profile. No scan above about 612,000 entries was measured, so the pruning budget at larger sizes is unverified.

## More suggestions from a scan (2026-11-07)

Requested by the owner after reviewing what took space on the development Mac. New discoveries, each a fixed kind in `catalog::DISCOVERIES`: `project_build_output`, `app_caches`, `ai_tool_old_versions`, `ai_tool_sessions`, `colima_vm`, `docker_desktop_vm`. The first four feed `Index::suggest`; listing in OMG Trash authorizes nothing, and every item still goes through a confirmed plan. Tier and action remain `review` or `info`, `action = "info"`.

Decisions:

- Build output needs proof: `target` beside `Cargo.toml`, `node_modules` beside `package.json`, a virtualenv with `pyvenv.cfg`. A folder merely called `target` is not suggested and stays an ordinary folder at deletion.
- Session transcripts cannot be recreated, so they carry a warning, need 7 days of age and 1 MB, and only `projects/<project>/*.jsonl` matches, never anything deeper such as memory.
- Container and virtual machine disks are refused by `policy::hard_protection` (docs/safety.md, "VM/container storage"). Before this change a `.colima` disk could be selected and deleted. The owner asked for these in the suggestions; they are shown as findings with the `docker system prune` route instead, because one disk holds every image, container and volume.
- `Index::auto_trash` is on by default on every platform (owner decision 2026-11-07); a user's "off" is kept. A scan fills the list, and deleting still needs a separately confirmed plan (S01, S02).
- `unused_clone` (owner request, 2026-11-07) suggests a whole repository only when `policy::pristine_clone` proves it: a remote, every branch equal to its remote branch, no loose objects (a clone keeps fetched objects in packs, so these mean a commit, a staged change or a pull), no stash, loose tag, submodule, worktree or operation in progress, no change time in the working tree after the index was written (except tool output such as `node_modules`), no unreadable folder, and 30 days untouched. This reads Git's own config and references, which is the one exception to metadata-only scanning; Git itself is never run. It can miss files that Git ignores if they were created before the index was last written, and a repository that was pulled may not qualify. No check shows that the remote still exists.
- A plan of several items refuses whole repositories (`Kind::StaleRepo`) and session transcripts with the reason "delete it on its own" (owner decision 2026-11-08), so "delete everything listed" never takes them; a plan of one item still can. Enforced in `prepare_delete`, not in the interface.
- `.dmg` files stay protected as disk images, so downloaded installers are not suggested.

Verification on the development Mac (macOS 27.2, Rust 1.95.0): `cargo test --workspace --locked` in both builds, clippy `-D warnings`, the PTY tests and `release.test.js`. New tests in `tests/cleanup.rs` cover proof and size floors, container disks, tool versions and transcripts; these run on Linux and macOS, and the application cache test on macOS only. Read-only on the real index of that Mac: `omg findings` lists the new rows.

Not passed yet: no measured use on Linux or Windows beyond the test suites; session transcripts on a real index (none on that Mac were old enough); Docker Desktop's folder, which was not present on that Mac.

## Apple and Windows signatures in the signing step (2026-11-13)

`scripts/platform-sign.js`, used by `sign-release.js` through `--apple-identity` with `--notary-profile`, and `--windows-signer`. Off by default; a release signed without them is unchanged. The platform signature is applied to a copy before the Ed25519 manifest, which covers the final bytes. An Apple identity without a notary profile is refused, as is a Developer ID signature without the hardened runtime or a timestamp, notarization that is not `Accepted`, and a Windows signer that leaves no certificate table.

Verification on the development Mac (macOS 27.2): `node --test packages/oh-my-gigabytes/test/release.test.js` (a fake Windows signer, a signer that signs nothing, Apple identity without notarization, certificate table parsing); an ad-hoc identity on a real `omg` binary was refused as "not a Developer ID Application certificate".

Not passed yet: no Developer ID certificate or Windows code-signing certificate exists, so real notarization, `spctl`, `Get-AuthenticodeSignature` and a signed binary passing `omg`'s own launch verification are untested. Releases stay unsigned by Apple and Microsoft until then.

