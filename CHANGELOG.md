# Changelog

Notable changes to OMG (Oh My Gigabytes). Every release is a development preview; full notes are on the [releases page](https://github.com/akramcodez/oh-my-gigabytes/releases).

## Unreleased

- **Docker build cache with several items.** Cleaning two or more build cache records failed with `Docker refused (400)` for each, because the engine accepts only one id per prune request. Each record is now pruned on its own, and an engine refusal shows Docker's reason.
- **Auto delete.** Off by default. From the options menu (`m`) or `omg auto-delete on --every 7`, after a permission screen, OMG deletes what has waited unchanged in OMG Trash for 1 day, 7 days, 1 month or 6 months, in the background after the computer starts and at most once a day. Changed items wait again, `K` keeps an item out, whole repositories, application data and toolchains need a second permission, and by default it runs only on mains power. A notification follows each run that deleted something, and the Deleted tab lists those items under "Auto-deleted". Turning it off takes effect at once, even during a run.
- **`D` empties OMG Trash.** After its one confirmation it deletes every listed item, whole repositories and application data included; their warnings are shown in the plan first. `omg clean` still never takes those two. Protected items, missing creation times and the checks before each removal are unchanged.
- **Removed suggestions come back.** A cache you took out of OMG Trash is suggested again once it has doubled or grown by 1 GB, or after 30 days. Suggestions removed with an earlier version return on the next scan.
- **`omg trash --add PATH` and `--remove PATH`** add a scanned path to OMG Trash or take it out from the shell, under the same rules as the interface. Nothing moves or is deleted.
- **The interface explains a partial scan.** It shows how many folders could not be read and, on macOS, which privacy setting to allow.
- **More developer leftovers.** Suggested: Poetry, PDM, Hatch, pipx and Yarn 2+ caches, Bazel output bases (Linux), SwiftUI preview data and watchOS, tvOS and visionOS device support files. Listed in Findings only: Simulator devices, Android system images, Vagrant boxes and user-installed .NET SDKs unchanged for 90 days, and VS Code and Cursor compiled code of older builds.
- **The Findings tab shows a total** at the top: how many items are listed and their size, like OMG Trash. Protected items are not counted in the size.
- **Why each item waits.** `omg auto-delete status --items` lists each item in OMG Trash as due, waiting (with the days left), kept, changed or left for you, and OMG Trash shows the same as a tag on each row while auto delete is on.
- **Auto delete in `omg doctor`**: whether it is on, the period, the last run and whether the job is registered. With `--state-dir`, doctor shows only jobs for that folder and no longer shows the default profile's service unit.
- **A job whose omg is gone is caught.** After `npm uninstall` or a Node version switch under nvm, the auto delete and daily scan jobs can point at a missing binary. `omg doctor`, `omg auto-delete status`, `omg schedule status` and the interface now say so; `omg auto-delete on` registers the job again at the current path.
- **Uninstall steps** in the README and `docs/release.md`: run `omg auto-delete off` and `omg schedule disable` before `npm uninstall`.
- **Fixes.** `omg tree` on an excluded folder (`~/.ssh`, `OneDrive`) reports its size as unknown (`null`), `excluded` and `incomplete: true` instead of 0 bytes. The Linux refusal of a link as the scan root names the link. The scan's `estimated_total` (the progress estimate) is the last scan's count for the same root, or the volume's only when the root is a whole volume, instead of the whole volume for any folder.
- **Clearer refusal from `omg clean`** for a whole project or application data: it says to delete it from OMG Trash in the interface.

## 0.1.0 (2026-11-21)

- Much wider detection: build output of more ecosystems, about 20 more tool caches, Xcode data, downloaded ML models, old toolchain, JetBrains and editor extension versions, old installers, older copies of build folders, logs and temp files on macOS and Windows, and unused Docker data removed through Docker itself.
- Old Git clones are offered only when every tracked file matches `.git/index` exactly and nothing is local.
- Old sessions of terminal coding assistants.
- `omg unexplained`, `omg grew` and `omg catalog draft PATH`.
- Interface themes, a Deleted view, `U` to clear a list, and deleting a selection around protected items inside it.
- `omg clean --category ...` with `--confirm FINGERPRINT`, and `omg schedule enable` for a daily scan on macOS and Windows.
- Locked items and cloud placeholders are refused before the question; APFS name folding cannot get around protection.

## 0.1.0-beta.2 (2026-11-08)

- Second public beta.

## 0.1.0-beta.1 (2026-11-08)

- First public beta: scanning on Linux, Windows and macOS, findings, OMG Trash and permanent delete after a confirmed plan.
