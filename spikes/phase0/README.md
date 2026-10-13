# Phase 0 native metadata feasibility probe

This dependency-free Python experiment tests Linux kernel primitives before choosing the Rust traversal/platform implementation. **It is not the Rust engine, `omg` CLI, policy engine, or production scanner.** It has no cleanup handler and cannot certify Trash or cloud nonhydration.

Prerequisites: Python 3.12+, Linux x86_64/aarch64 with `openat2` allowed (Linux 5.6+). Unsupported kernels/platforms fail closed; do not replace this with weaker path resolution to get a green test.

From repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 spikes/phase0/metadata_probe.py
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s spikes/phase0 -p 'test_*.py' -v
```

The executable creates synthetic fixtures in a temporary directory, inspects metadata with `O_PATH`/`openat2`/`fstat`, returns a redacted JSON result, then removes its own fixtures. Fixture writes/removal are test setup/teardown, not application cleanup. The probe deliberately attempts a read on an `O_PATH` handle to demonstrate that it is not content-readable; the read must fail with `EBADF` and never returns contents.

Checks cover hard-link identity/link count, sparse apparent/allocation distinction, parent timestamp counterexample, metadata-only handles, final/ancestor/substituted symlink rejection, root escape, replacement identities and a visible `.git` descendant fixture. Tests additionally cover lossless non-UTF-8 paths, invalid/missing paths, stable acquired identity after replacement and unsupported platform denial.

An acquired descriptor remains tied to an object after its pathname changes. This does **not** make a later pathname-based Trash operation use that object. No mutation primitive, recursive policy, live cloud/mount substitution test or resource-bounded traversal is established here.

Observed environment/result: [docs/phase0.md](../../docs/phase0.md). Benchmark fixture design: [docs/phase0-validation.md](../../docs/phase0-validation.md). Do not interpret test passing or CI success as native Windows/macOS certification or developer validation.
