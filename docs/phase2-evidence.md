# Phase 2 implementation evidence

Status: **inventory-only catalog/policy development milestone implemented; full Phase 2 acceptance incomplete.** Native tool-version qualification and production release trust-root provisioning remain open.

## Implemented contracts

- Strict bounded TOML (256 KiB, at most 64 rules), schema/engine/version validation, stable IDs/revisions, platform/tool compatibility labels, explanations, effects, regeneration, warnings, fixtures and sources.
- Four independently authored bundled rules: npm `_cacache`, pip cache, uv cache, large-file human review. They match known default roots or a stated logical-size threshold and explicitly leave tool versions/active use unknown. No Safe rule, Trash or command action qualifies.
- Core policy separately denies known credentials/databases/VM/partial/cloud paths, unknown identity/types and repository scopes. Filename policy is conservative, not a claim to identify every renamed secret or live database.
- Finding fingerprints bind metadata, scope, rule ID and catalog bytes; recomputed evaluation avoids trusting stale cached safety decisions. Exact/subtree ignores stored locally with boundary-aware matching and removal.
- Signed offline import verifier: exact domain-separated payload, Ed25519 strict verification, SHA-256 catalog identity, schema/action validation, monotonic anti-replay, atomic activation, previous-catalog rollback and bounded retention.
- Import is disabled in normal installs until an owner-controlled release verification key is provisioned; no placeholder key is trusted. Tests provision synthetic keys locally. No network update mechanism or arbitrary shell capability.
- Schema 1→2 migration, `omg findings`, `catalog status/import/rollback`, and ignore review/removal CLI.

## Verification

`cargo test --workspace --locked` and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed: 11 tests (3 catalog, 2 policy, 6 scanner). Tests cover strict parser/tier/action/text contracts; signed activation, tamper rejection, replay, rollback and last-good retention; protected-path/repository denial; ignore sibling boundaries. Synthetic finding fixtures exercise npm/pip/uv positive/near-miss, large review/protected database, evidence, exact ignore/removal and replacement-fingerprint invalidation.

## Limits and unpassed gates

- Tool-root overrides and native macOS/Windows layouts are not yet implemented; unsupported native scanner remains disabled. Known default-root match is inventory evidence, not dependency/removability proof.
- Findings are paged over indexed metadata and may produce an empty page; complete category/group aggregation is Phase 3 UI work. There is no actionable reclaimable estimate because all action handlers are disabled. *Milestone-time statement (2026-10-15 note):* Phase 3 later added permanent deletion (D15) and dry-run previews; see [phase3-evidence.md](phase3-evidence.md).
- Release key governance/provisioning/rotation and re-verification of retained signed artifacts after local-state tampering need release hardening. The owner-controlled SQLite state is not proof against same-account compromise.
- Fixtures currently are synthetic engine tests; no real supported tool version is promoted to actionable compatibility. No rule executes a tool or reads contents during scanning.
- Core protections/fixture tests reduce obvious scope errors but full adversarial cross-platform policy/race tests remain required before actions.

This milestone can support read-only review work. It does not satisfy the full traceable, native-tested catalog acceptance contract or authorize cleanup.

## Directory-scoped findings (added after Phase 5)

Directory-root rules (`npm_content_cache`, `pip_cache`, `uv_cache`) now yield **one finding per cache root directory**, resolved directly rather than by paging the largest entries. The estimate is the deduplicated subtree total; a directory whose subtree contains any protected, excluded or uncertain entry (or that policy itself denies, including anything inside a Git repository) is `protected`. `large_file` findings come from an indexed query and skip files inside a directory finding, so estimates are not double counted; the dry-run planner also drops targets nested inside another selected target. Catalog tiers and actions are unchanged (inventory only, no cleanup). This closes the "deduplicate candidate targets/estimates" checklist item for the current rule set; real tool/version qualification of the rules is still pending.

## More suggestions from a scan (2026-11-07)

Requested by the owner after reviewing what took space on the development Mac. New discoveries, each a fixed kind in `catalog::DISCOVERIES`: `project_build_output`, `app_caches`, `ai_tool_old_versions`, `ai_tool_sessions`, `colima_vm`, `docker_desktop_vm`. The first four feed `Index::suggest`; listing in OMG Trash authorizes nothing, and every item still goes through a confirmed plan. Tier and action remain `review` or `info`, `action = "info"`.

Decisions:

- Build output needs proof: `target` beside `Cargo.toml`, `node_modules` beside `package.json`, a virtualenv with `pyvenv.cfg`. A folder merely called `target` is not suggested and stays an ordinary folder at deletion.
- Session transcripts cannot be recreated, so they carry a warning, need 7 days of age and 1 MB, and only `projects/<project>/*.jsonl` matches, never anything deeper such as memory.
- Container and virtual machine disks are refused by `policy::hard_protection` (docs/safety.md, "VM/container storage"). Before this change a `.colima` disk could be selected and deleted. The owner asked for these in the suggestions; they are shown as findings with the `docker system prune` route instead, because one disk holds every image, container and volume.
- `Index::auto_trash` is on by default on every platform (owner decision 2026-11-07); a user's "off" is kept. A scan fills the list, and deleting still needs a separately confirmed plan (S01, S02).
- `.dmg` files stay protected as disk images, so downloaded installers are not suggested.

Verification on the development Mac (macOS 27.2, Rust 1.95.0): `cargo test --workspace --locked` in both builds, clippy `-D warnings`, the PTY tests and `release.test.js`. New tests in `tests/cleanup.rs` cover proof and size floors, container disks, tool versions and transcripts; these run on Linux and macOS, and the application cache test on macOS only. Read-only on the real index of that Mac: `omg findings` lists the new rows.

Not passed yet: no measured use on Linux or Windows beyond the test suites; session transcripts on a real index (none on that Mac were old enough); Docker Desktop's folder, which was not present on that Mac.
