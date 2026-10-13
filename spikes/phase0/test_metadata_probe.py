import errno
import os
import platform
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import metadata_probe as probe


@unittest.skipUnless(platform.system() == "Linux" and platform.machine() in
                     ("x86_64", "aarch64"), "native Linux probe only")
class MetadataFeasibilityTests(unittest.TestCase):
    def test_synthetic_probe_invariants(self):
        report = probe.run_probe()
        self.assertTrue(report["passed"], report["checks"])
        self.assertEqual(len(report["checks"]), 11)
        self.assertIn("trash_execution", report["capability_denials"])

    def test_native_non_utf8_filename_roundtrip(self):
        with tempfile.TemporaryDirectory(prefix="omg-phase0-") as tmp:
            root_fd = os.open(tmp, os.O_PATH | os.O_DIRECTORY)
            try:
                file_path = os.fsencode(tmp) + b"/artifact-\xff"
                fd = os.open(file_path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
                os.close(fd)
                result = probe.observe(root_fd, b"artifact-\xff")
                self.assertEqual(result["identity"][1], os.lstat(file_path).st_ino)
            finally:
                os.close(root_fd)

    def test_root_and_invalid_path_inputs_cannot_escape(self):
        with tempfile.TemporaryDirectory(prefix="omg-phase0-") as tmp:
            fd = os.open(tmp, os.O_PATH | os.O_DIRECTORY)
            try:
                for path in ("", "/", "/etc/passwd", b"artifact\0suffix"):
                    with self.subTest(path=path), self.assertRaises(ValueError):
                        probe.acquire_metadata_fd(fd, path)
                with self.assertRaises(OSError) as error:
                    probe.acquire_metadata_fd(fd, "../")
                self.assertEqual(error.exception.errno, errno.EXDEV)
            finally:
                os.close(fd)

    def test_missing_target_is_not_retargeted(self):
        with tempfile.TemporaryDirectory(prefix="omg-phase0-") as tmp:
            fd = os.open(tmp, os.O_PATH | os.O_DIRECTORY)
            try:
                with self.assertRaises(FileNotFoundError):
                    probe.observe(fd, "missing")
            finally:
                os.close(fd)

    def test_acquired_identity_survives_pathname_replacement(self):
        with tempfile.TemporaryDirectory(prefix="omg-phase0-") as tmp:
            root = Path(tmp)
            (root / "artifact").write_bytes(b"old")
            root_fd = os.open(root, os.O_PATH | os.O_DIRECTORY)
            fd = probe.acquire_metadata_fd(root_fd, "artifact")
            try:
                old_identity = os.fstat(fd).st_ino
                (root / "artifact").rename(root / "old-artifact")
                (root / "artifact").write_bytes(b"new")
                self.assertEqual(os.fstat(fd).st_ino, old_identity)
                self.assertNotEqual(probe.observe(root_fd, "artifact")["identity"][1],
                                    old_identity)
            finally:
                os.close(fd)
                os.close(root_fd)

    def test_unvalidated_platform_has_no_fallback(self):
        with patch.object(probe.platform, "system", return_value="Windows"):
            with self.assertRaises(probe.UnsupportedCapability):
                probe.acquire_metadata_fd(-1, "artifact")


if __name__ == "__main__":
    unittest.main()
