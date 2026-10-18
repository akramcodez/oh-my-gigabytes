"""Native Linux PTY smoke test of the built Rust CLI; no user data scanned."""
import os
import pty
import fcntl
import select
import struct
import subprocess
import tempfile
import termios
import time
import unittest
from pathlib import Path


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


if __name__ == "__main__":
    unittest.main()
