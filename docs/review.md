# Reviewing the deletion code

A guide for an outside reviewer of the only code in omg that removes user data. The rules it must keep are S01 to S12 in [safety.md](safety.md); where this page and that one differ, safety.md wins.

## What to read

| File | What it does |
| --- | --- |
| `crates/omg-core/src/cleanup.rs` | The single deletion path. `verdict` decides whether a path may be deleted at all; `prepare_delete` builds a plan and its fingerprint; `delete_permanently` re-checks and removes. `select_for_clean` and `clean_selection` are the scripted `omg clean` front end and end in the same two calls. |
| `crates/omg-core/src/policy.rs` | `hard_protection` (credentials, keys, databases, disk images, `.git`, `.ssh`, cloud folders, container disks), `repository_status`, `pristine_clone`. |
| `crates/omg-core/src/platform/macos.rs` | `removal_hazard`: iCloud placeholders, locked files. |
| `crates/omg-core/src/docker.rs` | Docker cleanup. `check_request` is the allow-list of engine requests; `Engine::remove` re-checks the whole plan and each item. |
| `crates/omg-core/src/scheduled.rs` | The daily job on macOS and Windows. It must only ever run `omg scan`. |

Nothing else may remove, rename or unlink user files. `planning::execute` always fails, and the service sources are checked by the test `service_logic_contains_no_deletion_or_cleanup_calls`.

## Threat model

Who and what can act while omg runs:

- **Another process of the same user** changes the tree between scan, plan and delete: swaps a folder for a link, renames a parent, puts a protected file inside a folder that was plain. Expected defence: removal goes through directory descriptors opened without following links; identity (`dev:ino`, or volume and file id on Windows) and birth time are checked again right before removal, and descendants are walked again under protection rules. A mismatch skips the item.
- **A stale or edited index.** The SQLite index is a cache and can be wrong or tampered with by the user. It must never be authority on its own: every item is checked against the live file system before removal.
- **A signed catalog.** Catalog rules can only explain; the parser admits tiers `info` and `review` and action `info`. Check that no catalog field reaches `verdict`.
- **A local Docker engine.** Only a local socket owned by the user or root, or a Windows named pipe; remote `DOCKER_HOST` is refused. Only ping, disk usage, removal of one image or one stopped container by validated id without force, and build cache prune by id.
- **Scripts.** `omg clean` deletes nothing without `--confirm` and a fingerprint computed from the exact findings selected; a different selection produces a different fingerprint.

Out of scope: a hostile root user, a compromised omg binary, and data shared by hard links, APFS clones or snapshots (space freed is an estimate).

## Questions worth answering

1. Can any path outside the home folder reach `delete_permanently`?
2. Can a link, junction, mount point or reparse point be followed during removal?
3. Can an item replaced after `prepare_delete` be removed?
4. Can a protected file inside a folder be removed with its parent?
5. Can a catalog, config value, service event or suggestion authorize removal (S02)?
6. Does the audit row in `operations` always match what happened, including partial and cancelled runs (S10)?
7. Can `check_request` be made to send a request outside its allow-list, or `Engine::remove` remove something not in the confirmed plan?
8. Can the scheduled job be pointed at another program or given other arguments?

## Running the tests

```sh
cargo test -p omg-core --test cleanup        # Linux and macOS: verdicts, plans, races, removal
cargo test -p omg-core --test docker         # fake engine: allow-list and plan checks
cargo test -p omg-core --test windows        # Windows: delete by handle
cargo test -p omg-core --test service service_logic_contains   # Linux only
cargo test -p omg-core --lib scheduled
cargo test --workspace --no-default-features --locked   # build without any deletion
```

On a Mac, set `TMPDIR=$(cd "$TMPDIR" && pwd -P) OMG_REQUIRE_DISK_IMAGES=1` first. Report findings as described in [SECURITY.md](../SECURITY.md).
