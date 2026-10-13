# Rules catalog design

Status: proposed TOML schema and authoring workflow. Product requirements: [fullcontext.md](fullcontext.md). Policy: [safety.md](safety.md).

## 1. Responsibilities

A rule identifies **what was observed**, **why it is a candidate**, **what evidence supports that interpretation**, and **what approved action could apply**. It does not authorize execution. Core policy computes the effective outcome and may downgrade or deny an otherwise matched rule.

Catalog versions are independent of binary versions. New catalog releases can expand descriptions and supported bounded discovery within builtin capabilities; adding a new executable adapter requires a binary release and safety tests.

## 2. Required schema fields

| Field | Contract |
| --- | --- |
| `schema_version` | Supported catalog schema integer |
| `id`, `name`, `rule_version` | Stable identifier, user-readable name, monotonic rule revision |
| `platforms`, `tool_versions`, `app_versions` | Explicit applicable ranges or justified not-applicable value; unknown versions never imply compatibility |
| `category`, `tier` | Valid enum; requested tier cannot override policy |
| `discovery` | Builtin adapter ID or anchored permitted patterns, supported root resolver, entry type |
| `evidence` | Required typed predicates, sources, freshness, and fail-closed behavior |
| `explanation`, `side_effects`, `regeneration`, `warnings` | Plain-English nonempty descriptions including uncertainty |
| `action` | `info`, `trash`, or supported builtin cleanup adapter; no command string |
| `preconditions`, `privileges`, `exclusions`, `failure_behavior` | Validated predicates and explicit skip/deny outcomes |
| `compatibility`, `fixtures`, `deprecation` | Tested targets/tool versions, fixture references, lifecycle status and replacement if any |

Use strict `serde` parsing with unknown-field rejection, collection/string/depth/byte limits, semantic enum validation, and no SQL/executable expression language. Paths resolve relative to trusted builtin roots, not arbitrary variable expansion.

## 3. Example: informational npm cache rule

This illustrative TOML is a schema proposal, not a shipped or tested rule. It deliberately has no cleanup action because this research has not established safe execution semantics.

```toml
schema_version = 1
id = "dev.npm.content-cache.inventory"
name = "npm package download cache"
rule_version = 1
platforms = ["linux", "macos", "windows"]
tool_versions = ["11.x"]
app_versions = ["not-applicable"]
category = "package_caches"
tier = "info"
explanation = "npm stores downloaded package data here to avoid downloading it again."
side_effects = ["Removing cache data can require downloads during future installs."]
regeneration = "npm can recreate cache entries when packages are fetched."
warnings = ["Offline installs may rely on previously cached packages."]
privileges = "current-user"
preconditions = ["scope_resolved", "supported_layout", "metadata_only"]
exclusions = ["protected_data", "cloud_or_redirection", "active_downloads"]
failure_behavior = "informational-with-limitations"
fixtures = ["npm-layout-positive", "npm-root-with-credentials", "npm-reparse-negative"]

[discovery]
adapter = "npm_cache_inventory_v1"
root = "configured_user_npm_cache"
relative_patterns = ["_cacache"]
entry_type = "directory"

[evidence]
required = ["validated_cache_root", "known_layout_metadata", "no_content_reads"]
version_source = "explicit-validated-tool-version-or-unknown"
unknown_version = "unsupported-informational"

[action]
kind = "info"

[compatibility]
status = "experimental"
tested_platforms = []
tested_tool_versions = []

[deprecation]
deprecated = false
reason = ""
replacement = ""
```

Production rules need real fixture files and native compatibility evidence. A version range named in an example is not certification. Empty compatibility tests forbid Safe/action activation.

## 4. Matching pipeline

1. Verify the catalog and validate every rule before activation.
2. Resolve permitted roots with builtin platform adapters/configuration. Configured arbitrary paths do not become trusted tool storage just by being specified.
3. Query indexed metadata for discovery predicates within those roots, without reading file contents.
4. Attach typed evidence with provenance, observation time, scan coverage, native flags, and catalog/rule fingerprint.
5. Run mandatory policy. Missing evidence, unknown identity, unsupported layout/version, incomplete coverage, protected descendants, and cloud/redirection yield deny/info with a reason.
6. Deduplicate overlapping candidates/identity sets; prefer the more specific explanation without double-counting or weakening policy.
7. Apply local scoped ignores to visibility. Retain policy and audit invariants.
8. Revalidate all actionable evidence when preparing and executing a plan.

Cached evaluation keys include catalog digest, rule version, relevant metadata/evidence fingerprint, policy version, and scope/exclusion fingerprint. Invalidate on any change. Cache hits improve discovery speed, not destructive-operation authorization.

## 5. Initial catalog priorities

| Category | Initial approach | Evidence/action gate |
| --- | --- | --- |
| npm/pip caches | Developer inventory and regeneration/offline warnings | Tool root/layout/version; narrow scope; reviewed adapter later |
| Temporary files/logs | Known tool-specific leaves only; broad temp/log cleanup deferred | Proven provenance, no credentials/database/active work, tested regeneration |
| Installers/downloaded packages | Review of explicit files in user-approved download scope | Known installer type/path, not partial/in-use/cloud/protected; identity-safe Trash |
| OS installation leftovers | Informational | Official supported utility only after a dedicated adapter qualifies |
| Runtimes/toolchains | Informational initially | Metadata age cannot establish nonuse/dependency absence |
| Project build artifacts | Informational inside repos; deferred cleanup | Reviewed generated-artifact evidence and policy extension; never repo deletion |
| iOS backups | Informational first; later Review if precise adapter qualifies | Backup structure/tool semantics, consequences and active backup checks |
| Screenshots/downloads by age | Later Review, never Safe on age alone | Human judgment and exact eligible target scope |
| Large unchanged files | Analysis; Review only for policy-eligible paths | Human judgment; no size/age-based Safe promotion |
| Duplicates | Deferred opt-in content analysis | Size/hash/byte verification contract; preserve at least one item, hard links/shared storage and protected scope |

No category quota or “number of rules” target justifies unsafe coverage. Publish unsupported categories honestly.

## 6. Authoring and qualification

For every rule:

- Cite official tool/platform documentation and exact supported versions.
- Build synthetic positive, near-miss, protected-descendant, substituted-path, active-work, incomplete-scan, and unknown-version fixtures.
- Document regeneration cost, offline dependency, user modifications, locks, and irreversible effects.
- Require deterministic evidence and fail-closed handling. Explanations must not say “unused” when only age is known.
- Classify experimental/informational first. Promote to Review/Safe only with the rule and policy integration tests on declared environments.
- Independently review Safe promotions and new action scopes; record rationale and compatibility evidence.

Metadata-only evidence cannot determine actual project abandonment, every live database, or complete process nonuse. If those properties are required and cannot be established, no action qualifies.

## 7. Authenticated catalog update protocol

Recommended initial mechanism: signed offline import, then optional explicit online update using the same verifier.

1. Ship a known-good catalog and trusted verification keys in the app.
2. Sign an exact-byte, domain-separated manifest containing catalog/schema/engine compatibility versions, monotonic sequence, rule-file digests/sizes, and key ID. Do not verify one representation then parse a different canonicalization.
3. Fetch/import into a private bounded staging directory. Reject oversized manifests/files, archive traversal, symlinks, duplicate paths, unsupported encodings, redirects outside documented policy, and malformed signatures.
4. Verify trusted signature, all payload digests, engine/schema constraints, root boundaries, builtin action IDs, and fixture/compatibility metadata before activation.
5. Activate the fully validated catalog atomically and durably record its version/digest. Never replace last known-good with a partial update.
6. Keep the previous catalog for explicit rollback. Normal updates reject older sequence replay. Explicit user-requested rollback must verify an older signed artifact and record the decision; it is not automatic replay acceptance.
7. Key rotation requires an old-root-authorized transition or an authenticated binary update. Never trust a replacement key simply included in an untrusted download.
8. Offline operation uses bundled/last valid catalog. Invalid updates produce an actionable error without breaking analysis.

Use a reviewed format/key-management implementation. `ed25519-dalek` supplies signature primitives, not a complete update security system. If multiple release channels/keys increase complexity, evaluate a maintained update framework before inventing additional trust machinery.

No network request includes local roots, findings, filenames, or user decisions. Online update policy is configurable and initially explicit/opt-in; startup does not block on network access.

## 8. Ignore scopes

Allow exact `(rule ID, native path, optional volume identity)` and explicitly confirmed subtree scopes. Display scope before saving; “ignore this item” must not silently hide every finding from the rule. Ignore review/removal is available in the TUI. A moved/replaced volume does not inherit an exact-volume ignore accidentally. Adding an exclusion schedules discovery invalidation; removing an ignore does not select or authorize a target.
