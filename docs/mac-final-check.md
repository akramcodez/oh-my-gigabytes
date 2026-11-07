# macOS final check before the beta

A hands-on pass for the owner's Apple silicon Mac, to run on the commit that will become `0.1.0-beta.1`. The automated suite and a first real-Mac pass already ran on 2026-11-01 ([macos-evidence.md](macos-evidence.md), "Real Mac"). This pass repeats them on the merged tree (macOS work plus the Linux and Windows delete release, 2026-11-02), and adds what has never been tested on real hardware: Full Disk Access, iCloud placeholder files, a real external drive, other terminal apps, and the binary that CI actually builds for release.

Since 2026-11-07 the macOS build can permanently delete, on APFS and only after you confirm. Step 8b is the only step that deletes, and only inside a throwaway `HOME`. Never press `d` in the other steps. Scanning reads names, sizes and times, never file contents. Use a private state folder for every experiment (`--state-dir`, below) so your normal omg data is not touched.

**Stop and report** (see [SECURITY.md](../SECURITY.md)) if any step changes, moves or downloads a file, or follows a link out of the scan, or if a delete removes anything other than what you confirmed.

Tick each box as you go. Write down anything marked *record*; the last section says where it goes.

## 0. Prepare

```sh
cd ~/path/to/oh-my-gigabytes
git pull --ff-only
git log -1 --oneline        # record: the commit you are testing
sw_vers; uname -m           # record: macOS version and arm64
```

- [x] The commit is the release candidate (the latest `main` with green `Checks` on all nine legs).
- [x] Xcode Command Line Tools are installed (`xcode-select -p` prints a path).
- [x] `hdiutil info | grep omg-` prints nothing (no disk image left mounted from an earlier test run).

## 1. Automated suite (the same steps as CI's macOS legs)

```sh
scripts/mac-check.sh --smoke
```

It installs Rust and Node 22 into your home folder if they are missing (no sudo), then runs fmt, clippy and the tests in both builds, the terminal tests, the npm launcher tests and a smoke run on real paths. It mounts small disk images (APFS, HFS+, exFAT, FAT32) and detaches them at the end.

- [x] The summary at the end is all `PASS`.
- [x] Record: test counts from the summary, and the `omg scan --root $HOME` line from the smoke part (entries, unreadable folders, time).
- [x] `hdiutil info | grep omg-` prints nothing afterwards.

## 2. Build the release binary and set up a private state folder

```sh
cargo build --release -p omg-cli --locked
OMG="$PWD/target/release/omg"
S=$(mktemp -d /private/tmp/omg-final.XXXXXX)   # private state folder (mode 700)
"$OMG" --version
"$OMG" --state-dir "$S" doctor
```

- [ ] `--version` prints `omg 0.1.0-beta.1`.
- [ ] `doctor` prints a line starting `permanent delete: development-unqualified`, and `"cleanup": "development-unqualified"` in its JSON.

## 3. What omg must refuse

```sh
"$OMG" --state-dir "$S" scan --root /            # sealed system volume
"$OMG" --state-dir "$S" scan --root /tmp         # /tmp is a link to /private/tmp
mkdir -m 755 /private/tmp/omg-open && "$OMG" --state-dir /private/tmp/omg-open status; rmdir /private/tmp/omg-open
"$OMG" --state-dir "$S" start
```

- [x] `/` is refused.
- [x] `/tmp` is refused, and the message names `/private/tmp`.
- [x] A state folder that others can read (mode 755) is refused.
- [x] `start` (the background service) says it is not supported on this platform.

## 4. Your home folder without Full Disk Access

Use the terminal app you normally use, **without** Full Disk Access (System Settings › Privacy & Security › Full Disk Access: your terminal app is off or not listed).

```sh
time "$OMG" --state-dir "$S" scan --root "$HOME"
"$OMG" --state-dir "$S" scan --root "$HOME" --accept-gaps
"$OMG" --state-dir "$S" tree
"$OMG" --state-dir "$S" findings | head -80
```

- [x] The first scan finishes as partial and is not published; the message explains macOS privacy protection and where to allow the terminal app.
- [ ] If macOS asks whether your terminal may access Desktop, Documents or Downloads, answer **Don't Allow** for this step (it tests the unreadable case). Record which questions appeared.
- [x] With `--accept-gaps` it is published as partial.
- [ ] In `tree`, Desktop, Documents and Downloads are `unknown` with `"incomplete": true`, never `0.00 B`; `.ssh` and `.Trash` are `"excluded": true`.
- [x] Findings look sensible: developer caches (npm, pip, Cargo, Homebrew if you use them) and large files, each with an explanation; nothing under Mail, Messages, Safari or app containers is offered as a cleanup candidate.
- [x] Record: entries, unreadable folders, time.

## 5. Your home folder with Full Disk Access (never tested before)

Turn on Full Disk Access for your terminal app (System Settings › Privacy & Security › Full Disk Access), quit the terminal app completely and open it again, then set `OMG` and `S` again as in step 2 (same folder, or a new one).

```sh
time "$OMG" --state-dir "$S" scan --root "$HOME"
"$OMG" --state-dir "$S" tree
```

- [ ] The scan completes, or reports far fewer unreadable folders than in step 4. Record: entries, unreadable folders, time, and any folder that is still unreadable.
- [ ] Mail, Messages, Safari, Photos library and app containers now have sizes but stay protected (in Findings their tier is `protected`, or they are not listed as candidates).
- [ ] Turn Full Disk Access off again afterwards if you do not want your terminal to keep it.

## 6. iCloud placeholder files (never tested before)

Only if you use iCloud Drive with "Optimise Mac Storage", or Desktop and Documents in iCloud. Pick one file that is in the cloud only (Finder shows a cloud icon; or right-click › Remove Download to make one).

```sh
ls -lO "path/to/that/file"      # the flags column should include "dataless"
"$OMG" --state-dir "$S" scan --root "$HOME" --accept-gaps
ls -lO "path/to/that/file"      # still "dataless"
```

- [ ] The file is still `dataless` after the scan, and Finder still shows the cloud icon: omg did not download it.
- [ ] Note: `~/Library/Mobile Documents` (the iCloud Drive folder itself) is excluded by name and never opened, so test with a file under Desktop or Documents if those are in iCloud.

## 7. A real external drive

Plug in a USB stick or drive formatted exFAT or FAT32 (and APFS or HFS+ if you have one). Use the name Finder shows under `/Volumes`.

```sh
"$OMG" --state-dir "$S" scan --root "/Volumes/NAME"
"$OMG" --state-dir "$S" tree
```

- [ ] The scan completes and the total is close to Finder's "Get Info" (Finder counts in decimal GB, omg in GiB).
- [ ] If you have a network share (SMB) mounted: `scan --root /Volumes/SHARE` is refused as not local.

## 8. The interface

Run it in Terminal.app, and again in any other terminal you use (iTerm2, Ghostty, the VS Code terminal):

```sh
"$OMG" --state-dir "$S"
```

- [ ] `1`–`4` switch between Overview, Browse, Findings and OMG Trash.
- [ ] In Browse, `⏎` opens a folder and `⌫` goes back. Inside a nested folder the rows show plain names (`victim.bin`, not `work/victim.bin`) and the path bar shows one piece per folder (`~ › Library › Caches`).
- [ ] Unreadable folders (step 4 index) say `unknown · unreadable`, not `0.00 B`.
- [ ] The footer, the `m` options menu and the `?` help list delete. Do not press `d` here; step 8b tests it.
- [ ] `␣` (space) adds the row to OMG Trash, and pressing it again takes it out; `"$OMG" --state-dir "$S" trash` agrees, and nothing changed on disk.
- [ ] `r` rescans and shows progress; the result line mentions privacy protection if folders were unreadable.
- [ ] Making the window very small shows "Terminal too small"; making it large again redraws correctly.
- [ ] `q` quits and the terminal is normal afterwards (prompt, cursor and typing all fine). Same for Ctrl-C.
- [ ] Record: which terminal apps you tried, and anything that looked wrong (a screenshot helps).

## 8b. Delete, in a throwaway home

```sh
T=$(mktemp -d /private/tmp/omg-home.XXXXXX)
mkdir -p "$T/stuff" "$T/Desktop" "$T/Tools/Thing.app/Contents"
echo a > "$T/stuff/remove-me.txt"; echo b > "$T/stuff/keep-me.txt"; echo c > "$T/Desktop/note.txt"
echo d > "$T/locked.txt"; chflags uchg "$T/locked.txt"; echo e > "$T/Tools/Thing.app/Contents/x"
HOME="$T" "$OMG" --state-dir "$S" scan --root "$T"
HOME="$T" "$OMG" --state-dir "$S"
```

- [ ] `d` on `stuff/remove-me.txt`, then Esc or Cancel: the file is still there.
- [ ] `d` again, "Yes, delete permanently": that file is gone and `keep-me.txt` is still there.
- [ ] `d` on `Desktop/note.txt` warns about iCloud before you confirm.
- [ ] `d` on `locked.txt`, on `Tools/Thing.app` and on `Desktop` itself removes nothing and says why.
- [ ] If you use iCloud with "Optimise Mac Storage": a cloud-only file in Desktop or Documents is refused and keeps its cloud icon. Use your real `HOME` and a real scan for this one, press `d` only on that file, and answer the question only if it offers no "Yes".
- [ ] Clean up: `chflags nouchg "$T/locked.txt"; rm -rf "$T"`

## 9. The binary CI builds for the release, installed like a user would

This checks the exact `darwin-arm64` binary from the `Release build` workflow (built on a hosted macOS 15 runner), signed with a throwaway key on your Mac and installed through npm. It is also the dry run of the changed `release.yml` that [beta-release-progress.md](beta-release-progress.md) still lists as not done. Needs `gh` 2.49 or newer, logged in.

```sh
gh workflow run release.yml --ref main
gh run list --workflow release.yml --limit 1          # wait until it shows "completed success"; note RUN_ID
W=$(mktemp -d /private/tmp/omg-pkg.XXXXXX) && cd "$W"
gh run download RUN_ID --repo akramcodez/oh-my-gigabytes --dir ci-artifacts
cat ci-artifacts/omg-darwin-arm64/CLEANUP             # must say: development-unqualified
node -e "
  const c = require('node:crypto'), fs = require('node:fs');
  const { publicKey, privateKey } = c.generateKeyPairSync('ed25519');
  fs.mkdirSync('keys', { mode: 0o700 });
  fs.writeFileSync('keys/public.pem', publicKey.export({ type: 'spki', format: 'pem' }));
  fs.writeFileSync('keys/private.pem', privateKey.export({ type: 'pkcs8', format: 'pem' }), { mode: 0o600 });"
node ~/path/to/oh-my-gigabytes/scripts/sign-release.js --artifacts ci-artifacts --version 0.1.0-beta.1 \
  --public-key keys/public.pem --private-key keys/private.pem --out signed --repo akramcodez/oh-my-gigabytes
npm install --global --prefix "$W/prefix with space" --ignore-scripts --offline --no-audit --no-fund \
  signed/tarballs/oh-my-gigabytes-0.1.0-beta.1.tgz signed/tarballs/oh-my-gigabytes-darwin-arm64-0.1.0-beta.1.tgz
"$W/prefix with space/bin/omg" --version
"$W/prefix with space/bin/omg" --state-dir "$S" doctor
"$W/prefix with space/bin/omg" --state-dir "$S" scan --root "$HOME" --accept-gaps
xattr -l "$W/prefix with space/lib/node_modules/oh-my-gigabytes-darwin-arm64/artifact/omg"
```

The throwaway key is only for this test; it is not the release key and must not be used for a release.

- [x] The `Release build` run is green for all four targets. Record: the run id.
- [ ] `sign-release.js` verifies the attestation and signs without errors; the macOS manifest says `cleanup: development-unqualified`.
- [ ] The installed `omg` runs from the npm prefix (the path has a space on purpose), `doctor` says `permanent delete: development-unqualified`, and the scan works.
- [ ] `xattr -l` on the installed binary shows no `com.apple.quarantine`, and macOS showed no "cannot be verified" dialog.
- [x] Change one byte of the installed binary (`printf x >> ".../artifact/omg"`) and run `omg --version` again: it must refuse with "Binary integrity verification failed". Then remove the test folder: `cd ~ && rm -rf "$W"`.

Known and expected: a binary downloaded **in a browser** is quarantined and, because the beta is not notarized, macOS blocks it with "Apple could not verify…" until it is allowed in System Settings. The npm install path above is not affected.

## 10. Clean up

```sh
rm -rf "$S" /private/tmp/omg-final.* /private/tmp/omg-pkg.*
hdiutil info | grep omg- || echo "no omg disk images mounted"
```

- [ ] Full Disk Access for the terminal is set back the way you want it.

## Where the results go

Add a section "Final check (DATE)" to [macos-evidence.md](macos-evidence.md) with the environment (Mac model, macOS version, terminal apps), the commit, and for each step: pass, or what happened. Then update the macOS lines in [beta-release-progress.md](beta-release-progress.md) ("Not passed yet") and, if step 9 ran, the `Release build` run id. Anything that failed is a release blocker until it is fixed or explicitly accepted by the owner.

Not covered by this pass and still open after it: Intel Macs (not a release target), macOS older than 15, Time Machine local snapshots, a second Mac, Apple notarization.
