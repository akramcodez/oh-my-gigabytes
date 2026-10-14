# Linux OS Trash: investigation (WP09)

Status: **investigation delivered; OS Trash remains disabled** (`capability::os_trash()` = `unsupported`). Date: 2026-10-15.

## Sources

- freedesktop.org Trash Specification, latest (fetched 2026-10-15 via `agent-reach` web channel, Jina Reader; source `https://specifications.freedesktop.org/trash/latest`). Key points below are from that page.
- Existing project constraints: `docs/safety.md` section 5 (TOCTOU and Trash), `crates/omg-core/src/cleanup.rs` (`delete_one`).

## What the specification requires

| Area | Requirement |
| --- | --- |
| Home trash | `$XDG_DATA_HOME/Trash` (default `~/.local/share/Trash`), with `files/`, `info/`, and `directorysizes`. |
| Other volumes | `$topdir/.Trash/$uid` (admin-created, sticky bit must be set and checks must pass) or `$topdir/.Trash-$uid` (created by the implementation). Fall back to the home trash if neither works. |
| Names in `files/` | Chosen by the implementation, but must be unique. A trashed copy must never overwrite an earlier one. |
| Original location | Stored only in `info/<name>.trashinfo`: `[Trash Info]`, `Path=` (absolute, or relative to `$topdir`), `DeletionDate=` (RFC 3339 local time). A name in `files/` must never be used to recover the original name. |
| Permissions | Check the user can delete the item before trashing. A copy across filesystems may not preserve permissions. Trashing can fail for permission reasons even when deletion would succeed. |
| Directory sizes | `directorysizes` is an optional cache, updated by writing a temp file and `rename()`-ing it into place. |
| Erasing | Not defined by the spec. It is a separate operation. |

## Why this is hard for OMG

1. **The race.** Trashing is a move by pathname. OMG validates the target by identity (`dev:ino` plus birth time) through a directory descriptor, but a later `rename` by name acts on whatever object has that name at the moment of the call. A substituted object can be moved. Passing a verified descriptor is not enough, because the move itself is path-based. This is the gap in `docs/safety.md` section 5.
2. **Staging does not close the gap by itself.** One option is `renameat2(RENAME_NOREPLACE)` into a private staging directory on the same filesystem, then compare identity, then move to `files/`. Problems: staging is itself a mutation of a possibly wrong object; moving it back could overwrite a replacement that appeared at the original name; and descendants can still change during the move. The safety document requires this to be proven with tests before any directory is trashed. It is not proven.
3. **Restore usability.** Staging must not make the only record of the original location a private temporary path. `info/` must record the real original path before the object is moved.
4. **Cross-volume.** Moving across filesystems is copy-then-delete, which is not atomic and cannot preserve everything. Same-volume `$topdir/.Trash` needs the sticky-bit and ownership checks, and those must be tested on each supported mount layout (ext4, Btrfs including subvolumes, tmpfs, WSL drvfs).
5. **Space.** Moving to Trash does not free space. The UI must say so, and OMG must never auto-empty Trash.
6. **Library.** The plan mentions choosing a maintained crate. The `trash` crate's path-based API has the same race, and `docs/safety.md` also notes non-thread-safe mount enumeration in that family of crates. No crate was adopted.

## Decision

- Keep OS Trash **disabled** and report it as `unsupported` in `omg doctor`, the status JSON and the release manifest (already done in WP02).
- A release may ship analysis plus D15 permanent delete (`development-unqualified`). It may not describe anything as "moved to Trash" or "recoverable".
- Re-open this package only with a design that binds the move to the verified object (for example, a descriptor-based move on which both the check and the mutation depend), plus controlled race tests on ext4, Btrfs and tmpfs.

## Test plan for a future attempt (not implemented)

- Substitution between identity check and move (file and directory).
- Concurrent writer inside a directory being trashed.
- Crash after staging and before `info/` is written; crash after `info/` and before the move.
- Name collision in `files/`; `info/` write failure; read-only or missing `.Trash`.
- Restore from the spec-defined layout, with the original `Path` correct, and no overwrite of a replacement at the original location.
- Raw (non-UTF-8) names in `Path=` and in `files/`.
