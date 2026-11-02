"""Windows ConPTY tests of the built CLI: the interface starts and quits, and permanent delete removes
only what was confirmed.

Needs `pip install pywinpty` and a built binary (target/debug/omg.exe, or OMG_BINARY). No user data
is scanned: every test scans a throwaway folder, and the delete tests point USERPROFILE (omg's home
on Windows) and LOCALAPPDATA at it for the omg process only.
"""
import os
import queue
import subprocess
import tempfile
import threading
import time
import unittest
from pathlib import Path

from terminal_screen import Screen


def built_binary():
    default = Path(__file__).resolve().parents[1] / "target" / "debug" / "omg.exe"
    return Path(os.environ.get("OMG_BINARY") or default)


class Interface:
    """The interface of a built omg in a ConPTY, read through a Screen."""

    def __init__(self, binary, state, env=None, rows=40, cols=140):
        from winpty import PtyProcess

        self.screen = Screen(rows, cols)
        self.process = PtyProcess.spawn(
            [str(binary), "--state-dir", str(state)], dimensions=(rows, cols), env=env
        )
        self.chunks = queue.Queue()
        threading.Thread(target=self._pump, daemon=True).start()

    def _pump(self):
        try:
            while True:
                self.chunks.put(self.process.read(4096))
        except EOFError:
            pass

    def until(self, needle, seconds=20):
        """Feed the screen until it shows `needle` or the time is up; returns whether it did."""
        deadline = time.monotonic() + seconds
        while needle not in self.screen.text() and time.monotonic() < deadline:
            try:
                chunk = self.chunks.get(timeout=0.1)
            except queue.Empty:
                continue
            self.screen.feed(chunk)
            if "\x1b[6n" in chunk:
                self.process.write("\x1b[1;1R")  # answer a cursor-position query
        return needle in self.screen.text()

    def settle(self, seconds=1.0):
        """Keep reading for a while, so a late redraw (a replaced message, say) reaches the screen."""
        self.until("\0", seconds)

    def send(self, keys):
        self.process.write(keys)

    def wait(self, seconds=15):
        deadline = time.monotonic() + seconds
        while self.process.isalive() and time.monotonic() < deadline:
            time.sleep(0.1)
        return None if self.process.isalive() else self.process.exitstatus

    def close(self):
        if self.process.isalive():
            self.process.terminate(force=True)


@unittest.skipUnless(os.name == "nt", "ConPTY requires Windows")
class ConPtySmokeTests(unittest.TestCase):
    def test_cached_tui_starts_and_quits_cleanly(self):
        binary = built_binary()
        if not binary.exists():
            self.skipTest("cargo build -p omg-cli required")
        with tempfile.TemporaryDirectory(prefix="omg-conpty-") as directory:
            root = Path(directory) / "root"
            root.mkdir()
            state = Path(directory) / "state"
            subprocess.run(
                [str(binary), "--state-dir", str(state), "scan", "--root", str(root)],
                check=True,
                capture_output=True,
            )
            ui = Interface(binary, state, rows=24, cols=80)
            try:
                self.assertTrue(ui.until("OMG"), ui.screen.text())
                ui.send("q")
                self.assertEqual(ui.wait(), 0, "the interface did not quit cleanly on q")
            finally:
                ui.close()


@unittest.skipUnless(os.name == "nt", "ConPTY requires Windows")
class ConPtyDeleteTests(unittest.TestCase):
    """The Windows twins of the Linux PTY delete tests in test_terminal.py."""

    def start(self, directory):
        binary = built_binary()
        if not binary.exists():
            self.skipTest("cargo build -p omg-cli required")
        # The real path: omg compares the scan root and home by their final paths, and a temp
        # folder can be named through an 8.3 short name.
        home = (Path(directory) / "home").resolve()
        home.mkdir()
        if any((ancestor / ".git").exists() for ancestor in home.parents):
            self.skipTest(f"{home} is inside a Git repository; set TMP to a folder outside one")
        state = Path(directory) / "state"
        env = dict(os.environ, USERPROFILE=str(home), LOCALAPPDATA=str(home / "AppData" / "Local"))
        doctor = subprocess.run(
            [str(binary), "--state-dir", str(state), "doctor"], capture_output=True, text=True, env=env
        ).stdout
        if "permanent delete: development-unqualified" not in doctor:
            self.skipTest("this build or system offers no permanent delete")
        return binary, home, state, env

    def scan(self, binary, state, home, env):
        result = subprocess.run(
            [str(binary), "--state-dir", str(state), "scan", "--root", str(home)],
            capture_output=True,
            text=True,
            env=env,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('"state": "complete"', result.stdout)

    def test_delete_removes_only_the_confirmed_file_and_refuses_protected_ones(self):
        with tempfile.TemporaryDirectory(prefix="omg-delete-") as directory:
            binary, home, state, env = self.start(directory)
            work = home / "work"
            work.mkdir()
            victim, protected, keep = work / "victim.bin", work / "secret.pem", work / "keep.txt"
            victim.write_bytes(b"x" * (3 << 20))
            protected.write_bytes(b"k" * (1 << 20))
            keep.write_text("keep me")
            self.scan(binary, state, home, env)
            ui = Interface(binary, state, env)
            try:
                ui.send("2")
                self.assertTrue(ui.until("work"), ui.screen.text())
                ui.send("\r")
                self.assertTrue(ui.until("victim.bin"), ui.screen.text())
                # The question names the file, defaults to Cancel, and Esc leaves everything in place.
                ui.send("d")
                self.assertTrue(ui.until("Are you sure?"), ui.screen.text())
                self.assertTrue(ui.until("Yes, delete permanently"), ui.screen.text())
                self.assertIn("victim.bin", ui.screen.text())
                self.assertTrue(victim.exists())
                ui.send("\x1b")
                self.assertTrue(ui.until("Nothing was deleted"), ui.screen.text())
                # It stays: a later reply from the worker used to replace it with "Removed from OMG Trash."
                ui.settle()
                self.assertIn("Nothing was deleted", ui.screen.text())
                self.assertEqual(victim.stat().st_size, 3 << 20)
                # Confirming removes exactly that file.
                ui.send("d")
                self.assertTrue(ui.until("Are you sure?"), ui.screen.text())
                ui.send("2")
                self.assertTrue(ui.until("Deleted 1 item"), ui.screen.text())
                self.assertFalse(victim.exists())
                self.assertEqual(keep.read_text(), "keep me")
                self.assertTrue(protected.exists())
                # A credential file is refused with a reason, and no deletion is offered.
                self.assertTrue(ui.until("secret.pem"), ui.screen.text())
                ui.send("d")
                self.assertTrue(ui.until("cannot add to OMG Trash"), ui.screen.text())
                self.assertNotIn("Yes, delete permanently", ui.screen.text())
                self.assertTrue(protected.exists())
                ui.send("q")
                self.assertEqual(ui.wait(), 0)
            finally:
                ui.close()

    def test_a_cache_holding_a_protected_folder_is_deleted_around_it(self):
        with tempfile.TemporaryDirectory(prefix="omg-around-") as directory:
            binary, home, state, env = self.start(directory)
            # uv's documented Windows cache: %LOCALAPPDATA%\uv\cache.
            local = home / "AppData" / "Local"
            cache = local / "uv" / "cache" / "archive-v0"
            (cache / "aaa" / "lib").mkdir(parents=True)
            (cache / "aaa" / "lib" / "mod.py").write_bytes(b"x" * (3 << 20))
            (cache / "bbb" / "pkg" / "credentials").mkdir(parents=True)
            (cache / "bbb" / "pkg" / "credentials" / "auth.py").write_text("library code")
            (cache / "bbb" / "pkg" / "__init__.py").write_bytes(b"y" * (1 << 20))
            (cache / "ccc").mkdir()
            (cache / "ccc" / "data").write_bytes(b"z" * (1 << 20))
            self.scan(binary, state, home, env)
            ui = Interface(binary, state, env)
            try:
                ui.send("2")
                # Open each folder only once its row is listed, and wait for the path to show it.
                for folder in ("AppData", "Local", "uv"):
                    self.assertTrue(ui.until(folder), ui.screen.text())
                    ui.send("\r")
                    self.assertTrue(ui.until(f"› {folder}"), ui.screen.text())
                self.assertTrue(ui.until("cache"), ui.screen.text())
                ui.send("d")
                # Nothing is offered as a plain "Yes": the folder cannot go whole.
                self.assertTrue(ui.until("Cannot delete this whole"), ui.screen.text())
                self.assertTrue(ui.until("Delete everything except the protected items"), ui.screen.text())
                self.assertIn("protected item", ui.screen.text())
                self.assertNotIn("Yes, delete permanently", ui.screen.text())
                self.assertTrue((cache / "aaa").exists())
                ui.send("2")
                # The plan around the protected folder says what stays and asks again.
                self.assertTrue(ui.until("keeping 1 protected item"), ui.screen.text())
                self.assertTrue(ui.until("Yes, delete permanently"), ui.screen.text())
                self.assertTrue((cache / "aaa").exists(), "nothing is removed before the second confirmation")
                ui.send("2")
                self.assertTrue(ui.until("Deleted"), ui.screen.text())
                self.assertFalse((cache / "aaa").exists())
                self.assertFalse((cache / "ccc").exists())
                self.assertFalse((cache / "bbb" / "pkg" / "__init__.py").exists())
                self.assertEqual((cache / "bbb" / "pkg" / "credentials" / "auth.py").read_text(), "library code")
                ui.send("q")
                self.assertEqual(ui.wait(), 0)
            finally:
                ui.close()


if __name__ == "__main__":
    unittest.main()
