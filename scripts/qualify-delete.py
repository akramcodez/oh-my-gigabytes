#!/usr/bin/env python3
"""Qualify permanent delete (and Move to Trash) of a built omg on one filesystem, with throwaway files only.

    python3 scripts/qualify-delete.py --binary PATH [--root DIR] [--report FILE]

Each scenario makes a fresh home and state folder under DIR (default: the system temp folder),
scans it, drives the real interface in a terminal (a PTY on Linux and macOS, ConPTY on Windows,
which needs `pip install pywinpty==2.0.15`) and checks what is left on disk. omg only ever sees
the throwaway home: HOME (USERPROFILE and LOCALAPPDATA on Windows) points at it for omg alone.
Put DIR on the filesystem being qualified, for example a mounted image from scripts/fs-sandbox.sh.
Prints PASS or FAIL per scenario and exits 1 if any failed; --report also writes them as JSON.
"""
import argparse
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tests"))

WINDOWS = os.name == "nt"
if WINDOWS:
    from test_terminal_windows import Interface as _Interface
else:
    from test_terminal import Interface as _Interface

DOWN, ESC, ENTER = "\x1b[B", "\x1b", "\r"
MB = 1 << 20


class Interface(_Interface):
    def send(self, keys):
        super().send(keys if WINDOWS else keys.encode())

    def quit(self):
        self.send("q")
        if WINDOWS:
            return self.wait() == 0
        try:
            return self.process.wait(timeout=15) == 0
        except subprocess.TimeoutExpired:
            return False


def filesystem(path):
    path = str(Path(path).resolve())
    try:
        if WINDOWS:
            drive = path[0]
            out = subprocess.run(
                ["powershell", "-NoProfile", "-Command", f"(Get-Volume -DriveLetter {drive}).FileSystemType"],
                capture_output=True, text=True, timeout=30,
            ).stdout
            return out.strip() or "unknown"
        if sys.platform == "darwin":
            best = ("", "unknown")
            for line in subprocess.run(["/sbin/mount"], capture_output=True, text=True).stdout.splitlines():
                if " on " not in line or " (" not in line:
                    continue
                point = line.split(" on ", 1)[1].split(" (", 1)[0]
                kind = line.rsplit(" (", 1)[1].split(",", 1)[0].rstrip(")")
                if (path == point or path.startswith(point.rstrip("/") + "/")) and len(point) > len(best[0]):
                    best = (point, kind)
            return best[1]
        return subprocess.run(["stat", "-f", "-c", "%T", path], capture_output=True, text=True).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return "unknown"


def select(ui, name):
    """Move the cursor to the row named `name` in the folder listing."""
    def named(line):
        return re.search(rf"  {re.escape(name)}(/|\s|$)", line.rstrip()) is not None

    deadline = time.monotonic() + 15
    while not any(named(line) for line in ui.screen.text().splitlines()) and time.monotonic() < deadline:
        ui.settle(0.2)
    ui.settle(0.5)
    lines = ui.screen.text().splitlines()
    cursor = next((i for i, line in enumerate(lines) if line.lstrip().startswith("❯")), None)
    row = next((i for i, line in enumerate(lines) if cursor is not None and i >= cursor and named(line)), None)
    if row is None:
        raise AssertionError(f"no row named {name}")
    for _ in range(row - cursor):
        ui.send(DOWN)
        ui.settle(0.2)


def choice(ui, label):
    """The number the open dialog gives `label`, once it is drawn."""
    ui.until(label, 5)
    found = re.search(rf"(\d)\. {re.escape(label)}", ui.screen.text())
    if not found:
        raise AssertionError(f"no choice named {label}")
    return found.group(1)


class Run:
    def __init__(self, binary, root):
        self.binary, self.root = binary, root
        self.results = []

    def home(self):
        base = Path(tempfile.mkdtemp(prefix="omg-qualify-", dir=self.root)).resolve()
        home, state = base / "home", base / "state"
        (home / "q").mkdir(parents=True)
        if not WINDOWS:
            state.mkdir(mode=0o700)
        if WINDOWS:
            env = dict(os.environ, USERPROFILE=str(home), LOCALAPPDATA=str(home / "AppData" / "Local"))
        else:
            env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(base / "config"),
                       XDG_DATA_HOME=str(home / ".local" / "share"), TERM="xterm-256color")
        if sys.platform == "darwin":
            (home / ".Trash").mkdir()
        return base, home, state, env

    def omg(self, state, env, *args):
        return subprocess.run([str(self.binary), "--state-dir", str(state), *args], capture_output=True, text=True, env=env)

    def open(self, home, state, env, name):
        """Scan the throwaway home, open the interface in its folder q and wait for `name`."""
        scan = self.omg(state, env, "scan", "--root", str(home), "--no-suggest")
        if scan.returncode != 0 or '"state": "complete"' not in scan.stdout:
            raise AssertionError(f"scan failed: {scan.stdout}{scan.stderr}")
        ui = Interface(self.binary, state, env)
        try:
            ui.send("2")
            select(ui, "q")
            ui.send(ENTER)
            if not ui.until("~ › q"):
                raise AssertionError(ui.screen.text())
            select(ui, name)
        except Exception:
            ui.close()
            raise
        return ui

    def scenario(self, name, body):
        base, home, state, env = self.home()
        started = time.monotonic()
        ui = None
        try:
            ui, ok, note = body(base, home, state, env)
            if ok and not ui.quit():
                ok, note = False, "the interface did not quit cleanly"
            screen = "" if ok else ui.screen.text()
        except Exception as error:  # a failed step is a failed scenario, not a crash of the run
            ok, note, screen = False, f"{type(error).__name__}: {error}", ui.screen.text() if ui else ""
        finally:
            if ui:
                ui.close()
            shutil.rmtree(base, ignore_errors=True)
        self.results.append({"scenario": name, "pass": ok, "note": note, "seconds": round(time.monotonic() - started, 1)})
        print(("PASS " if ok else "FAIL ") + name + (f": {note}" if note else ""), flush=True)
        if screen:
            print(screen, flush=True)

    def confirm(self, ui):
        ui.send("d")
        if not ui.until("Are you sure?"):
            return False
        ui.send(choice(ui, "Yes, delete permanently"))
        return ui.until("Deleted ") or ui.until("skipped", 5)


def cancel_keeps_the_file(run):
    def body(base, home, state, env):
        big, small = home / "q/big.bin", home / "q/small.txt"
        big.write_bytes(b"x" * (3 * MB))
        small.write_text("keep")
        ui = run.open(home, state, env, "big.bin")
        ui.send("d")
        asked = ui.until("Are you sure?") and "big.bin" in ui.screen.text()
        ui.send(ESC)
        said = ui.until("Nothing was deleted")
        ui.settle()
        ok = asked and said and "Nothing was deleted" in ui.screen.text() and big.stat().st_size == 3 * MB
        return ui, ok, "" if ok else f"asked={asked} said={said} exists={big.exists()}"
    run.scenario("Cancel keeps the file", body)


def confirm_deletes_only_that_file(run):
    def body(base, home, state, env):
        big, small, outside = home / "q/big.bin", home / "q/small.txt", base / "outside.txt"
        big.write_bytes(b"x" * (3 * MB))
        small.write_text("keep")
        outside.write_text("outside")
        ui = run.open(home, state, env, "big.bin")
        deleted = run.confirm(ui) and "Deleted 1 item" in ui.screen.text()
        ok = deleted and not big.exists() and small.read_text() == "keep" and outside.read_text() == "outside"
        return ui, ok, "" if ok else f"deleted={deleted} big_exists={big.exists()}"
    run.scenario("Confirm deletes exactly the confirmed file", body)


def protected_file_is_refused(run):
    def body(base, home, state, env):
        key, small = home / "q/id.pem", home / "q/small.txt"
        key.write_bytes(b"k" * (3 * MB))
        small.write_text("keep")
        ui = run.open(home, state, env, "id.pem")
        ui.send("d")
        refused = ui.until("cannot add to OMG Trash")
        ok = refused and "Yes, delete permanently" not in ui.screen.text() and key.stat().st_size == 3 * MB
        return ui, ok, "" if ok else f"refused={refused} exists={key.exists()}"
    run.scenario("A credential file is refused without a question", body)


def folder_tree_is_deleted_whole(run):
    def body(base, home, state, env):
        tree, small = home / "q/tree", home / "q/small.txt"
        for i in range(20):
            folder = tree / f"d{i}" / "inner"
            folder.mkdir(parents=True)
            for j in range(10):
                (folder / f"f{j}.bin").write_bytes(b"t" * 16384)
        small.write_text("keep")
        ui = run.open(home, state, env, "tree")
        deleted = run.confirm(ui) and "Deleted 1 item" in ui.screen.text()
        ok = deleted and not tree.exists() and small.read_text() == "keep"
        return ui, ok, "" if ok else f"deleted={deleted} tree_exists={tree.exists()}"
    run.scenario("A folder of 220 items is deleted whole and nothing beside it", body)


def link_inside_is_not_followed(run):
    probe = Path(tempfile.mkdtemp(dir=run.root))
    try:
        os.symlink(probe / "nothing", probe / "link")
        can_link = True
    except OSError:
        can_link = False
    finally:
        shutil.rmtree(probe, ignore_errors=True)
    if not can_link:
        run.results.append({"scenario": "A link inside a deleted folder is not followed", "pass": None, "note": "skipped: this account cannot create symbolic links"})
        print("SKIP A link inside a deleted folder is not followed: this account cannot create symbolic links")
        return

    def body(base, home, state, env):
        tree, outside = home / "q/tree", base / "outside"
        tree.mkdir()
        (tree / "data.bin").write_bytes(b"x" * (3 * MB))
        outside.mkdir()
        (outside / "precious.txt").write_text("precious")
        os.symlink(outside, tree / "to-outside", target_is_directory=True)
        os.symlink(outside / "precious.txt", tree / "to-precious")
        ui = run.open(home, state, env, "tree")
        ui.send("d")
        asked = ui.until("Are you sure?") or ui.until("cannot add to OMG Trash", 3)
        if "Are you sure?" in ui.screen.text():
            ui.send(choice(ui, "Yes, delete permanently"))
            ui.until("Deleted ") or ui.until("skipped", 5)
        intact = (outside / "precious.txt").read_text() == "precious"
        outcome = "deleted" if not tree.exists() else "kept"
        note = f"folder {outcome}; the link targets outside home are intact" if intact else "a link target was changed"
        return ui, asked and intact, note
    run.scenario("A link inside a deleted folder is not followed", body)


def hard_link_keeps_the_other_name(run):
    def body(base, home, state, env):
        big, other = home / "q/big.bin", home / "other/big-link.bin"
        big.write_bytes(b"h" * (3 * MB))
        other.parent.mkdir()
        os.link(big, other)
        ui = run.open(home, state, env, "big.bin")
        deleted = run.confirm(ui) and "Deleted 1 item" in ui.screen.text()
        ok = deleted and not big.exists() and other.stat().st_size == 3 * MB and other.read_bytes()[:1] == b"h"
        return ui, ok, "" if ok else f"deleted={deleted} other_exists={other.exists()}"
    run.scenario("Deleting one hard link keeps the other name and its data", body)


def replaced_after_the_question_is_skipped(run):
    def body(base, home, state, env):
        big = home / "q/big.bin"
        big.write_bytes(b"x" * (3 * MB))
        ui = run.open(home, state, env, "big.bin")
        ui.send("d")
        asked = ui.until("Are you sure?")
        big.unlink()
        time.sleep(1.1)
        big.write_bytes(b"n" * (3 * MB))
        ui.send(choice(ui, "Yes, delete permanently"))
        ui.until("skipped") or ui.until("Deleted ", 5)
        text = ui.screen.text()
        kept = big.exists() and big.read_bytes()[:1] == b"n"
        ok = asked and kept and "skipped" in text
        return ui, ok, "" if ok else f"asked={asked} new_file_kept={kept}"
    run.scenario("A file replaced after the question is skipped, not deleted", body)


def move_to_trash_keeps_it_restorable(run):
    def body(base, home, state, env):
        big = home / "q/big.bin"
        big.write_bytes(b"t" * (3 * MB))
        ui = run.open(home, state, env, "big.bin")
        ui.send("d")
        asked = ui.until("Are you sure?")
        ui.send(choice(ui, "Move to Trash (can be restored)"))
        said = ui.until("to the Trash")
        trash = home / ".Trash" if sys.platform == "darwin" else home / ".local/share/Trash/files"
        moved = trash / "big.bin"
        kept = moved.exists() and moved.stat().st_size == 3 * MB and moved.read_bytes()[:1] == b"t"
        info = sys.platform == "darwin" or "Path=" in (home / ".local/share/Trash/info/big.bin.trashinfo").read_text()
        ok = asked and said and not big.exists() and kept and info
        return ui, ok, "" if ok else f"asked={asked} said={said} gone={not big.exists()} in_trash={kept} info={info}"
    run.scenario("Move to Trash keeps the data in the Trash", body)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--binary", required=True, type=Path, help="the omg binary to qualify")
    parser.add_argument("--root", type=Path, default=Path(tempfile.gettempdir()), help="a folder on the filesystem to qualify")
    parser.add_argument("--report", type=Path, help="also write the results to this JSON file")
    args = parser.parse_args()
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    binary, root = args.binary.resolve(), args.root.resolve()
    if any((ancestor / ".git").exists() for ancestor in [root, *root.parents]):
        sys.exit(f"{root} is inside a Git repository, where omg refuses to delete; use a folder outside one")
    version = subprocess.run([str(binary), "--version"], capture_output=True, text=True).stdout.strip()
    probe = Path(tempfile.mkdtemp(dir=root))
    doctor = subprocess.run([str(binary), "--state-dir", str(probe / "s"), "doctor"], capture_output=True, text=True).stdout
    shutil.rmtree(probe, ignore_errors=True)
    delete = next((line.split(":", 1)[1].strip() for line in doctor.splitlines() if line.startswith("permanent delete:")), "unknown")
    system = f"{platform.system()} {platform.release()} {platform.machine()}"
    print(f"{version} on {system}, filesystem {filesystem(root)} at {root}, permanent delete: {delete}", flush=True)
    if delete in ("unknown", "unsupported"):
        sys.exit("this build or filesystem offers no permanent delete")
    trash = re.search(r"OS Trash: ([a-z-]+)", doctor)
    trash = trash.group(1) if trash else "unknown"
    run = Run(binary, root)
    for scenario in (
        cancel_keeps_the_file,
        confirm_deletes_only_that_file,
        protected_file_is_refused,
        folder_tree_is_deleted_whole,
        link_inside_is_not_followed,
        hard_link_keeps_the_other_name,
        replaced_after_the_question_is_skipped,
        *([move_to_trash_keeps_it_restorable] if trash not in ("unknown", "unsupported") else []),
    ):
        scenario(run)
    failed = [r for r in run.results if r["pass"] is False]
    print(f"{len(run.results) - len(failed)} of {len(run.results)} passed or skipped, {len(failed)} failed")
    if args.report:
        args.report.write_text(json.dumps({
            "binary": version, "system": system, "filesystem": filesystem(root),
            "permanent_delete": delete, "os_trash": trash, "results": run.results,
        }, indent=2) + "\n")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
