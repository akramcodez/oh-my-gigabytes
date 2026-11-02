"""Native Linux PTY smoke test of the built Rust CLI; no user data scanned."""
import os
import pty
import fcntl
import select
import sqlite3
import struct
import subprocess
import tempfile
import termios
import time
import unittest
from pathlib import Path

from terminal_screen import Screen


class Interface:
    """The interface of a built omg in a pseudo-terminal, read through a Screen."""

    def __init__(self, binary, state, env, rows=40, cols=140):
        self.master, self.slave = pty.openpty()
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.screen = Screen(rows, cols)
        self.process = subprocess.Popen([str(binary), "--state-dir", str(state)], stdin=self.slave, stdout=self.slave, stderr=self.slave, env=env)

    def until(self, needle, seconds=15):
        """Feed the screen until it shows `needle` or the time is up; returns whether it did."""
        deadline = time.monotonic() + seconds
        while needle not in self.screen.text() and time.monotonic() < deadline:
            if select.select([self.master], [], [], .1)[0]:
                chunk = os.read(self.master, 65536)
                self.screen.feed(chunk)
                if b"\x1b[6n" in chunk:
                    os.write(self.master, b"\x1b[1;1R")
        return needle in self.screen.text()

    def settle(self, seconds=1.0):
        """Keep reading for a while, so a late redraw (a replaced message, say) reaches the screen."""
        self.until("\0", seconds)

    def send(self, data):
        os.write(self.master, data)

    def close(self):
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait()
        os.close(self.master)
        os.close(self.slave)


@unittest.skipUnless(os.name == "posix", "PTY requires Unix")
class TerminalSmokeTests(unittest.TestCase):
    def test_cached_tui_quit_restores_terminal(self):
        binary = Path(__file__).resolve().parents[1] / "target/debug/omg"
        if not binary.exists():
            self.skipTest("cargo build -p omg-cli required")
        with tempfile.TemporaryDirectory(prefix="omg-terminal-") as directory:
            root = Path(directory) / "root"
            root.mkdir()
            state = Path(directory) / "state"
            subprocess.run([str(binary), "--state-dir", str(state), "scan", "--root", str(root)], check=True, capture_output=True)
            master, slave = pty.openpty()
            # termios.tcsetwinsize needs Python 3.11; Ubuntu 22.04 ships 3.10.
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
            before = termios.tcgetattr(slave)
            env = dict(os.environ, TERM="xterm-256color")
            process = subprocess.Popen([str(binary), "--state-dir", str(state)], stdin=slave, stdout=slave, stderr=slave, env=env)
            try:
                output = b""
                deadline = time.monotonic() + 10
                while b"OMG" not in output and time.monotonic() < deadline:
                    if select.select([master], [], [], .1)[0]:
                        output += os.read(master, 65536)
                        if b"\x1b[6n" in output:
                            os.write(master, b"\x1b[1;1R")
                self.assertIn(b"OMG", output)
                os.write(master, b"q")
                self.assertEqual(process.wait(timeout=10), 0)
                self.assertEqual(termios.tcgetattr(slave), before)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                os.close(master)
                os.close(slave)

    def test_lock_held_without_a_scan_is_waiting_then_runs_the_requested_scan(self):
        # Housekeeping (pruning old scans) holds scan.lock with no scan recorded. The interface used
        # to claim "Another omg scan is running", show no progress and drop the user's scan.
        binary = Path(__file__).resolve().parents[1] / "target/debug/omg"
        if not binary.exists():
            self.skipTest("cargo build -p omg-cli required")
        with tempfile.TemporaryDirectory(prefix="omg-terminal-") as directory:
            root = Path(directory) / "root"
            root.mkdir()
            (root / "file").write_bytes(b"x" * 4096)
            state = Path(directory) / "state"
            subprocess.run([str(binary), "--state-dir", str(state), "scan", "--root", str(root)], check=True, capture_output=True)
            holder = os.open(state / "scan.lock", os.O_RDWR)
            fcntl.flock(holder, fcntl.LOCK_EX)
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 140, 0, 0))
            env = dict(os.environ, TERM="xterm-256color")
            process = subprocess.Popen([str(binary), "--state-dir", str(state)], stdin=slave, stdout=slave, stderr=slave, env=env)

            screen = Screen(30, 140)

            def read_until(needle, seconds):
                """Feed the screen until it shows `needle` (or the time is up); returns its text."""
                deadline = time.monotonic() + seconds
                while needle not in screen.text() and time.monotonic() < deadline:
                    if select.select([master], [], [], .1)[0]:
                        chunk = os.read(master, 65536)
                        screen.feed(chunk)
                        if b"\x1b[6n" in chunk:
                            os.write(master, b"\x1b[1;1R")
                return screen.text()

            def scans():
                database = sqlite3.connect(f"file:{state / 'index.sqlite'}?mode=ro", uri=True)
                try:
                    return [row[0] for row in database.execute("SELECT state FROM scans WHERE id > 1")]
                finally:
                    database.close()

            try:
                shown = read_until("no scan is running", 15)
                self.assertIn("Waiting for another omg task", shown, shown)
                self.assertNotIn("Another omg scan is running", shown, shown)
                self.assertEqual(scans(), [])
                os.write(master, b"r")
                shown = read_until("your scan starts after", 5)
                self.assertIn("your scan starts after", shown, shown)
                fcntl.flock(holder, fcntl.LOCK_UN)
                deadline = time.monotonic() + 20
                while "complete" not in scans() and time.monotonic() < deadline:
                    read_until("\0", .3)
                self.assertIn("complete", scans(), "the scan asked for while waiting never ran")
                os.write(master, b"q")
                self.assertEqual(process.wait(timeout=10), 0)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                os.close(holder)
                os.close(master)
                os.close(slave)

    def test_delete_removes_only_the_confirmed_file_and_refuses_protected_ones(self):
        # Linux and Windows builds can delete permanently. This drives the real interface in a throwaway
        # home folder: Cancel keeps the file, only the confirmed file goes, a credential file is refused.
        binary = Path(__file__).resolve().parents[1] / "target/debug/omg"
        if not binary.exists():
            self.skipTest("cargo build -p omg-cli required")
        with tempfile.TemporaryDirectory(prefix="omg-delete-") as directory:
            home = Path(directory) / "home"
            work = home / "work"
            work.mkdir(parents=True)
            victim, protected, keep = work / "victim.bin", work / "secret.pem", work / "keep.txt"
            victim.write_bytes(b"x" * (3 << 20))
            protected.write_bytes(b"k" * (1 << 20))
            keep.write_text("keep me")
            state = Path(directory) / "state"
            env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(Path(directory) / "config"), TERM="xterm-256color")
            doctor = subprocess.run([str(binary), "--state-dir", str(state), "doctor"], capture_output=True, text=True, env=env).stdout
            if "permanent delete: development-unqualified" not in doctor:
                self.skipTest("this build or system offers no permanent delete")
            subprocess.run([str(binary), "--state-dir", str(state), "scan", "--root", str(home)], check=True, capture_output=True, env=env)
            ui = Interface(binary, state, env)
            try:
                ui.send(b"2")
                self.assertTrue(ui.until("work"), ui.screen.text())
                ui.send(b"\r")
                self.assertTrue(ui.until("victim.bin"), ui.screen.text())
                # The question names the file, defaults to Cancel, and Esc leaves everything in place.
                ui.send(b"d")
                self.assertTrue(ui.until("Are you sure?"), ui.screen.text())
                self.assertIn("victim.bin", ui.screen.text())
                self.assertIn("Yes, delete permanently", ui.screen.text())
                self.assertTrue(victim.exists())
                ui.send(b"\x1b")
                self.assertTrue(ui.until("Nothing was deleted"), ui.screen.text())
                # It stays: a later reply from the worker used to replace it with "Removed from OMG Trash."
                ui.settle()
                self.assertIn("Nothing was deleted", ui.screen.text())
                self.assertEqual(victim.stat().st_size, 3 << 20)
                # Confirming removes exactly that file.
                ui.send(b"d")
                self.assertTrue(ui.until("Are you sure?"), ui.screen.text())
                ui.send(b"2")
                self.assertTrue(ui.until("Deleted 1 item"), ui.screen.text())
                self.assertFalse(victim.exists())
                self.assertEqual(keep.read_text(), "keep me")
                self.assertTrue(protected.exists())
                # A credential file is refused with a reason, and no deletion is offered.
                self.assertTrue(ui.until("secret.pem"), ui.screen.text())
                ui.send(b"d")
                self.assertTrue(ui.until("cannot add to OMG Trash"), ui.screen.text())
                self.assertNotIn("Yes, delete permanently", ui.screen.text())
                self.assertTrue(protected.exists())
                ui.send(b"q")
                self.assertEqual(ui.process.wait(timeout=10), 0)
            finally:
                ui.close()

    def test_a_cache_holding_a_protected_folder_is_deleted_around_it(self):
        # Python packages ship library folders called `credentials`; the whole cache used to be refused
        # only after the user confirmed. Now the dialog says so up front and offers to delete the rest.
        binary = Path(__file__).resolve().parents[1] / "target/debug/omg"
        if not binary.exists():
            self.skipTest("cargo build -p omg-cli required")
        with tempfile.TemporaryDirectory(prefix="omg-around-") as directory:
            home = Path(directory) / "home"
            cache = home / ".cache" / "uv" / "archive-v0"
            (cache / "aaa" / "lib").mkdir(parents=True)
            (cache / "aaa" / "lib" / "mod.py").write_bytes(b"x" * (3 << 20))
            (cache / "bbb" / "pkg" / "credentials").mkdir(parents=True)
            (cache / "bbb" / "pkg" / "credentials" / "auth.py").write_text("library code")
            (cache / "bbb" / "pkg" / "__init__.py").write_bytes(b"y" * (1 << 20))
            (cache / "ccc").mkdir()
            (cache / "ccc" / "data").write_bytes(b"z" * (1 << 20))
            state = Path(directory) / "state"
            env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(Path(directory) / "config"), TERM="xterm-256color")
            doctor = subprocess.run([str(binary), "--state-dir", str(state), "doctor"], capture_output=True, text=True, env=env).stdout
            if "permanent delete: development-unqualified" not in doctor:
                self.skipTest("this build or system offers no permanent delete")
            subprocess.run([str(binary), "--state-dir", str(state), "scan", "--root", str(home)], check=True, capture_output=True, env=env)
            ui = Interface(binary, state, env)
            try:
                ui.send(b"2")
                self.assertTrue(ui.until(".cache"), ui.screen.text())
                ui.send(b"\r")
                self.assertTrue(ui.until("uv"), ui.screen.text())
                ui.send(b"d")
                # Nothing is offered as a plain "Yes": the folder cannot go whole.
                self.assertTrue(ui.until("Cannot delete this whole"), ui.screen.text())
                # The box is drawn in pieces: wait for its last choice before judging it.
                self.assertTrue(ui.until("Delete everything except the protected items"), ui.screen.text())
                self.assertIn("protected item", ui.screen.text())
                self.assertNotIn("Yes, delete permanently", ui.screen.text())
                self.assertTrue((cache / "aaa").exists())
                ui.send(b"2")
                # The plan around the protected folder says what stays and asks again.
                self.assertTrue(ui.until("keeping 1 protected item"), ui.screen.text())
                self.assertTrue(ui.until("Yes, delete permanently"), ui.screen.text())
                self.assertTrue((cache / "aaa").exists(), "nothing is removed before the second confirmation")
                ui.send(b"2")
                self.assertTrue(ui.until("Deleted"), ui.screen.text())
                self.assertFalse((cache / "aaa").exists())
                self.assertFalse((cache / "ccc").exists())
                self.assertFalse((cache / "bbb" / "pkg" / "__init__.py").exists())
                self.assertEqual((cache / "bbb" / "pkg" / "credentials" / "auth.py").read_text(), "library code")
                ui.send(b"q")
                self.assertEqual(ui.process.wait(timeout=10), 0)
            finally:
                ui.close()


if __name__ == "__main__":
    unittest.main()
