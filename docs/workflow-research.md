# Developer workflow research: WSL, Windows, Linux and macOS

Research/observation date: 2026-10-13. This combines one consenting founder's workflow, read-only local metadata observations, and official platform/tool documentation. **It is not a representative survey or evidence of what a majority prefers.** No independent prototype session has occurred.

## Product scope clarification

The founder clarified that OMG must solve Linux and macOS developers' storage problems broadly. Their WSL/Windows observations below are one case study. They do not establish catalog priority, market prevalence or the primary UI for every platform. The shared value proposition is quick storage attribution plus evidence-backed consequences and explicit user control.

Research and validation must include native Linux and macOS tracks independently: package-heavy development, multi-project/generated artifacts, installed toolchains, application caches, and protected managed storage. WSL host/VHD distinctions remain relevant to the Windows/WSL adapter. No actual native Mac user interview or filesystem test has been performed yet.

## Founder workflow

Direct user report:

- Platforms: WSL and Windows.
- Current process: `df -h /`, notice storage consumption, then ask an AI which local caches/tools to inspect and how to clean them.
- Concern: repeated installation/use of AI tools and developer packages accumulates storage and the machine feels less capable.
- Main frustration: **time spent checking and identifying what occupies storage**.
- Permission: inspect `/home/akram`; Windows PowerShell/user location also made available for necessary checks.

Interpretation: the strongest job-to-be-done is “show me the biggest tool-owned storage, explain what it is, and reduce the repeated investigation.” Perceived performance degradation is a user report, not a diagnosis. Storage observations do not prove causation or guarantee performance improvements after cleanup.

## Local inspection method and limits

Used GNU `du` metadata accounting, same-filesystem traversal, and no default symlink dereference. The initial home scan excluded Git metadata, SSH/GPG/AWS/Azure credential roots, and Docker configuration/storage by name. Tool subroot checks used selected cache/editor/runtime locations; no configuration, credential, AI conversation or source file contents were opened. Directory names and byte counts are metadata and remain local; no inventory was sent with web requests.

This was an authorized research inspection using an existing utility, **not the OMG scanner or a certified cloud-safe traversal**. It does not prove race handling, memory/queue limits, cloud behavior, live data disposability or full disk coverage. No cleanup command ran. Windows checks queried volume capacity via `Get-CimInstance Win32_LogicalDisk` and existence/reparse attributes of a small allowlist of tool roots; there was no recursive Windows/OneDrive scan.

Each `du` process deduplicates hard links within its own invocation. Multiple commands were live observations at different times; subtrees can change and cross-command totals must not be added or treated as a consistent snapshot. Installed tools/environments and unknown application data are informational, not deletion candidates. A `.git` entry exists at the Linux home root: a future repository-aware policy must resolve that scope conservatively and cannot assume hidden tool storage is outside repository protection.

## Observed storage, anonymized summary

Rounded binary units; these are storage observations, **not estimated recoverable space**. Private project names/filenames are intentionally omitted from this repository document.

| Observed group | Approximate allocated size | Interpretation |
| --- | --- | --- |
| Antigravity server root | 4.49 GiB | Most observed bytes under server `bin`; installed versions, not generic cache |
| VS Code remote server root | 3.19 GiB | Extensions about 1.71 GiB, server binaries about 0.65 GiB, other data about 0.83 GiB |
| Antigravity IDE server root | 1.92 GiB | Binaries/extensions/data; independent installed server location |
| Cursor remote server root | 1.41 GiB | Predominantly server binaries |
| nvm root | 2.16 GiB | Predominantly installed Node versions; removal requires dependency judgment |
| uv cache | 2.33 GiB | Tool-managed cache; upstream explicitly forbids direct cache modification |
| npm root | 0.62 GiB | `_npx` about 588 MiB; `_cacache` about 46 MiB; do not assume the entire npm root is package-download cache |
| Composer cache | 172 MiB | Download/build regeneration and offline consequences require tool-specific evidence |
| pre-commit cache | 121 MiB | Managed tool environments/cache, not automatically disposable |
| pnpm application-data root, later observation | 7.34 GiB | Needs separation of store, runtimes and active tool data; entire parent is not a cleanup target |
| OpenCode application-data root | 623 MiB | May include valuable local sessions/indexes; informational, no blanket cleanup |

The initial excluded home observation was about 44.83 GiB; later `.local` observations differed substantially from the initial measurement. Therefore the report deliberately does not claim a stable home total or infer growth rate. A future inventory must expose observation time/churn and reconcile generations rather than compare live command totals as if atomic.

Windows shallow root probes found Ollama, Cargo/rustup, npm/pip and uv locations. Existence does not establish installed versions, sizes, active usage, or whether models/packages are removable. Docker inventory was not queried; its size remains unknown.

## Critical WSL/Windows distinction

Measured WSL `df -h /`: virtual filesystem about 1007 GiB total, 76 GiB used, 881 GiB available (rounded output).

Measured Windows native logical volumes at a separate time:

| Volume | Capacity | Available |
| --- | --- | --- |
| C: | about 200.1 GiB | about 12.60 GiB |
| D: | about 175.0 GiB | about 157.2 MiB |

Microsoft documents that the WSL VHD's maximum displayed capacity may exceed physical host storage. Thus “881 GiB available in Linux” does not mean Windows can allocate another 881 GiB. The native query is read-only operational evidence, not Windows filesystem/Trash certification. The WSL backing drive and VHD path were not established, and logical volumes are not automatically independent physical storage pools.

**Product change:** show separate “WSL virtual filesystem available” and “Windows host volume available” measurements, with clear identities/timestamps and an unknown backing relationship when not established. Never add their capacities together or directly delete/modify a live VHD. Physical host availability must remain visible for WSL users even when virtual free-space percentage looks healthy.

## Platform workflow synthesis

| Platform | Source-supported workflow | OMG implication |
| --- | --- | --- |
| Linux | GNOME Disk Usage Analyzer scans home/folders; scan speed depends on file count, tree depth and storage media | Cached scoped results, metadata accounting, useful progress and coverage; avoid repeated full investigation |
| macOS | Storage settings show volumes/containers and category breakdowns; System Data includes system-managed caches/VM/runtime data and cannot be managed from that category | Explain opaque categories, keep system/VM data informational, do not equate broad categories with removable bytes |
| Windows | Storage settings/Cleanup recommendations review categories and selected files; Recycle Bin retention is separate; cloud placeholders are supported | Explicit review with consequences, native host space measurements, no automatic cleanup or cloud hydration |
| WSL | Linux VHD capacity differs from physical Windows capacity; VS Code installs server/extensions inside WSL separately from the Windows UI | Dual-environment accounting, installed-server versus cache distinction, no VHD deletion shortcut |
| Cross-platform developer tools | npm/pip/uv/pnpm and model caches have separate layout, shared-storage and regeneration semantics | Tool-owned explanations and reviewed adapters; unknown tool roots stay informational |

Official support instructions document supported mechanisms, not majority user preferences. Some describe automatic deletion, elevation, broad permanent pruning or cloud offloading; OMG does not inherit those policies.

## Highest-value rule research additions

1. **uv:** default cache root is platform-dependent; cache layouts are versioned; upstream says never directly modify the cache. Any eventual operation must use a qualified builtin adapter respecting locks and version-specific effects, with offline/rebuild/shared-link warnings. Keep info-only until then.
2. **pnpm:** store pruning is an official operation and may require future downloads after branch changes. Version-specific store/project/shared-link semantics need review. A `.local/share/pnpm` parent includes more than a single proven store target.
3. **Remote editors/AI IDE servers:** distinguish installed binaries/extensions from caches and durable settings. Size/age/version duplicates do not prove that a server version is inactive or unwanted.
4. **AI session/application data:** protect conversations, credentials, databases and local work. Tool names are useful attribution, not cleanup evidence.
5. **Model caches:** Hugging Face documents shared blobs/snapshots and Windows no-symlink duplication; Ollama's model roots are platform-specific and may coexist with keys. Models are Review/info, not Safe based on size; never delete the whole parent.
6. **Docker:** official `docker system df` distinguishes shared and unique image data; volumes can contain valuable data even when not attached. Keep managed storage informational and do not prune in ordinary scans.

## Sources accessed

- [Microsoft: manage WSL disk space](https://learn.microsoft.com/en-us/windows/wsl/disk-space)
- [Microsoft: Windows storage cleanup workflow](https://support.microsoft.com/en-us/windows/free-up-drive-space-in-windows-85529ccb-c365-490d-b548-831022bc9b32)
- [Apple: Storage settings and System Data](https://support.apple.com/guide/mac-help/mchl3d437fbc/mac)
- [GNOME: home scan](https://help.gnome.org/users/baobab/stable/scan-home.html.en)
- [GNOME: factors affecting scan time](https://help.gnome.org/users/baobab/stable/problem-slow-scan.html.en)
- [VS Code: WSL server/extension architecture](https://code.visualstudio.com/docs/remote/wsl)
- [uv: cache safety, locks, layout/versioning](https://docs.astral.sh/uv/concepts/cache/)
- [pnpm: store commands and pruning](https://pnpm.io/cli/store)
- [Docker: disk inventory](https://docs.docker.com/reference/cli/docker/system/df/)
- [Docker: managed pruning effects and volume protection](https://docs.docker.com/engine/manage-resources/pruning/)
- [Hugging Face: shared cache/model management](https://huggingface.co/docs/huggingface_hub/guides/manage-cache)
- [Ollama: model/key paths and memory versus disk](https://docs.ollama.com/faq)

Sources are current live documentation and may describe versions newer than local installations. No listed destructive command was executed, and no upstream unattended/force deletion options become OMG features.

## Validation status

Actual founder pain/problem evidence is stronger now: repeated manual diagnosis consumes time, and a real WSL/host-capacity mismatch exists. Majority preference, actual prototype comprehension, repeat-use advantage and independent developer feedback remain unverified. Phase 0 closes under owner-approved D14 on source-backed research/design; [phase0-evidence-review.md](phase0-evidence-review.md) adds Linux/macOS evidence and assigns those deferred checks to later gates. Use [phase0-prototype.md](phase0-prototype.md) for later read-only review; do not synthesize user answers from internet documentation.
