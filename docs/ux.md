# User experience and CLI contract

Status: proposed interaction design. All destructive controls depend on [safety.md](safety.md).

## 1. Startup and main screen

`omg` opens the TUI when stdin/stdout are terminals. Restore terminal state on normal exit, panic, and interrupt. Nonterminal invocation prints useful guidance/status rather than starting raw mode or scanning an unexpected root.

First run: show configured scope (proposed default current user's home), exclusions/cloud limitations, and local index location; start analysis without implying any cleanup approval. No service/network catalog update is installed or enabled automatically.

Repeat run: render the last valid inventory immediately; show timestamp, scope, coverage, and stale/partial status; schedule bounded refresh if needed. No full synchronous scan or network wait blocks the first frame.

```text
OMG — Storage intelligence                       Scan: refreshing
Volume / storage pool | Capacity | Available | Last sample
Scope: user home; 2 roots complete, 1 inaccessible; cloud excluded
Last full reconciliation: ... | Findings may be stale

Potentially reclaimable (estimate): ...  Selected (estimate): ...
Category         Item / location      Tier      Estimate    Reason
Package caches   npm cache ...        Info      ...         ...
Downloads        installer ...        Review    ...         ...

Details / warnings / scan limitations
↑↓ navigate  Space select  Enter inspect  p preview  / search  ? help
```

Grouping and sorting: categories by estimated eligible bytes, candidates by size within category. Informational storage has a separate subtotal and is not included as actionable potentially reclaimable space. Show apparent versus allocated estimates and coverage limitations in details.

## 2. Keyboard interactions

| Key proposal | Action |
| --- | --- |
| Up/Down or `j`/`k` | Navigate |
| Left/Right | Collapse/expand category |
| Tab / Shift-Tab | Cycle panes |
| Space | Toggle eligible target; show reason when denied |
| Enter | Inspect explanation/evidence/warnings |
| `p` | Preview selected plan |
| `/` | Search path/name; filters for category/tier/minimum size |
| `o` | Open eligible directory/reveal location using fixed native integration |
| `i` | Ignore this recommendation with explicit scope |
| `I` | Review/remove ignore preferences |
| `r` / `R` | Refresh / full reconciliation |
| `c` | Cancel active scan/operation where safe |
| `h` | Cleanup history and target outcomes |
| `?` | Full keyboard help |
| Esc | Cancel dialog/back; default confirmation answer is cancel |
| `q` | Quit with clear handling of active work |

Bindings are provisional; keep a persistent legend and separate dialog focus. Selection does not execute anything. Info/protected findings never get an enabled cleanup action. All targets start unselected in the MVP. Candidate updates preserve selection only when identity/evidence/action fingerprint remains unchanged; otherwise deselect with an explanation.

Category selection affects only eligible listed candidates; indicate exclusions and exact target count. Selecting a group is not consent to later members. Selected total is a union of nonoverlapping target identities and updates immediately.

## 3. Details and preview

Details include exact escaped path, type/identity availability, rule/catalog version, evidence, observation time, effective tier/policy reason, apparent/allocated estimate, regeneration, side effects, uncertainty, and ignore scope.

Preview is a dedicated screen containing:

- Plan ID and version/freshness.
- Item count and full target list or expandable explicitly scoped groups.
- Action for every target and exact builtin command/arguments for any future utility.
- Estimated size, shared/hard-link/allocation caveats, and available-space sample.
- Warnings about redownloads/offline installs, irreversible effects, permissions, and Trash retention.
- Skipped/denied targets and reasons.

Full paths remain inspectable on narrow screens; never authorize based on a truncated path. A changed target set/scope requires a regenerated preview.

## 4. Confirmation and execution

Use a separate confirmation step after preview. Cancel is focused by default. Require an explicit confirmation action/phrase, not the navigation key that opened the preview. Bind confirmation to the immutable plan fingerprint in the current session.

During execution, show current action, completed/skipped/failed counts, cancellation limitations, and no claim of final recovered space. Do not execute on a timer or automatically after a scan. Revalidate each target again; unsafe changes are skipped or invalidate the plan as defined by policy.

Result screen separates:

1. Planned and attempted targets.
2. Completed Trash/adapter operations.
3. Skipped, failed, cancelled, and unresolved/interrupted items.
4. Processed size estimate.
5. Observed available-space change, including zero/negative/unknown values and measurement times.

Explain that Trash normally retains blocks and that concurrent system activity makes the measured delta nonattributable. Do not use “freed” as a synonym for “processed.” Offer native Trash location/recovery guidance without promising undo.

## 5. CLI commands

Required public surface:

```sh
omg
omg scan
omg scan --full
omg start
omg stop
omg status
omg doctor
omg cleanup --dry-run
```

Bare `omg cleanup --dry-run` requires explicit target input; it does not mean “all candidates.” Proposed explicit selector:

```sh
omg cleanup --dry-run --candidate <candidate-id>
```

The placeholder is documentation notation; IDs are obtained from the application. Repeatable selectors or an exported plan may be supported later, but a plan file is untrusted data, not authorization. No implicit current-directory/all-safe cleanup.

Recommended first CLI cleanup contract: dry-run only. Actual cleanup occurs through the interactive TUI. If direct execution is later supported, require terminal review and a separate session-bound confirmation. Do not add `--yes`, force deletion, or a pipeable authorization token that creates unattended cleanup. Noninteractive analysis/dry-run is allowed; destructive nonterminal requests fail clearly.

Proposed extensions to settle during implementation:

- `omg scan --root <path>` for explicit scope changes with boundary validation.
- `omg catalog import <signed-bundle>` and `omg catalog update` for explicit verified updates.
- `omg service uninstall` to remove user service registration; `stop` does not imply uninstall.
- Machine-readable status/dry-run output with paths redacted by default and explicit local opt-in for details.

These extensions are not in the supplied command list and are not implemented. Until Phase 5, `start`/`stop` report service unavailability rather than simulate a healthy background process.

Define stable exit statuses for success, invalid input, incomplete scan, unavailable capability, operation failures, and cancellation when implementing the CLI. Successful process exit must not hide partial scan coverage; dry-run must report targets denied/skipped separately.

## 6. Terminal usability

- Support at least 80×24 as a primary baseline and exercise 60×20/120×40; actual certification uses tests. For smaller screens, switch to one-pane views rather than cutting off confirmation details.
- At extremely small sizes, show a resize/help screen with quit/cancel controls; never accept destructive confirmation while critical details are inaccessible.
- Convey tiers and state with words/symbols, not color alone. Provide monochrome/high-contrast styles.
- Respect Unicode grapheme width, lossless raw paths, and safe escaped display of invalid/control names. Do not emit embedded terminal escape sequences.
- Mouse is optional; every core action remains keyboard accessible.
- Preserve focus while background data arrives; do not move the cursor to newly sorted items unexpectedly.
- Open locations, never arbitrary file execution. Cloud/protected/unsupported location-opening limitations must be clear.
- History displays user-approved target detail locally, while diagnostic logs remain redacted by default.

## 7. Notifications and feedback

Future service notifications say only that a configured storage threshold was crossed, using a nonsensitive volume label. They have hysteresis/cooldown and never propose automatic cleanup. Optional user feedback is explicit local preference or voluntarily submitted reporting; no background analytics or upload channel is introduced.
