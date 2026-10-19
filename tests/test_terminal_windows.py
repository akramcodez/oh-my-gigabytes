"""Windows ConPTY smoke test of the built CLI: the interface starts, draws its banner and quits on q.

Needs `pip install pywinpty` and a built binary (target/debug/omg.exe, or OMG_BINARY). No user data
is scanned: the scan root is an empty folder.
"""
import os
import subprocess
import tempfile
import threading
import time
import unittest
from pathlib import Path


@unittest.skipUnless(os.name == "nt", "ConPTY requires Windows")
class ConPtySmokeTests(unittest.TestCase):
    def test_cached_tui_starts_and_quits_cleanly(self):
        from winpty import PtyProcess

        default = Path(__file__).resolve().parents[1] / "target" / "debug" / "omg.exe"
        binary = Path(os.environ.get("OMG_BINARY") or default)
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
            process = PtyProcess.spawn(
                [str(binary), "--state-dir", str(state)], dimensions=(24, 80)
            )
            output = []

            def pump():
                try:
                    while True:
                        output.append(process.read(4096))
                except EOFError:
                    pass

            threading.Thread(target=pump, daemon=True).start()
            try:
                deadline = time.monotonic() + 20
                while "OMG" not in "".join(output) and time.monotonic() < deadline:
                    if "\x1b[6n" in "".join(output):
                        process.write("\x1b[1;1R")  # answer a cursor-position query
                    time.sleep(0.1)
                self.assertIn("OMG", "".join(output))
                process.write("q")
                deadline = time.monotonic() + 15
                while process.isalive() and time.monotonic() < deadline:
                    time.sleep(0.1)
                self.assertFalse(process.isalive(), "the interface did not quit on q")
                self.assertEqual(process.exitstatus, 0)
            finally:
                if process.isalive():
                    process.terminate(force=True)


if __name__ == "__main__":
    unittest.main()
