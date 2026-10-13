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
- Findings are paged over indexed metadata and may produce an empty page; complete category/group aggregation is Phase 3 UI work. There is no actionable reclaimable estimate because all action handlers are disabled.
- Release key governance/provisioning/rotation and re-verification of retained signed artifacts after local-state tampering need release hardening. The owner-controlled SQLite state is not proof against same-account compromise.
- Fixtures currently are synthetic engine tests; no real supported tool version is promoted to actionable compatibility. No rule executes a tool or reads contents during scanning.
- Core protections/fixture tests reduce obvious scope errors but full adversarial cross-platform policy/race tests remain required before actions.

This milestone can support read-only review work. It does not satisfy the full traceable, native-tested catalog acceptance contract or authorize cleanup.

## Directory-scoped findings (added after Phase 5)

Directory-root rules (`npm_content_cache`, `pip_cache`, `uv_cache`) now yield **one finding per cache root directory**, resolved directly rather than by paging the largest entries. The estimate is the deduplicated subtree total; a directory whose subtree contains any protected, excluded or uncertain entry (or that policy itself denies, including anything inside a Git repository) is `protected`. `large_file` findings come from an indexed query and skip files inside a directory finding, so estimates are not double counted; the dry-run planner also drops targets nested inside another selected target. Catalog tiers and actions are unchanged (inventory only, no cleanup). This closes the "deduplicate candidate targets/estimates" checklist item for the current rule set; real tool/version qualification of the rules is still pending.
