#!/usr/bin/env python3
"""Linux-only Phase 0 feasibility experiment, not the OMG storage engine.

Only synthetic fixture metadata is inspected. No scan API, content reads,
cleanup handler, or claim of race-safe Trash execution is supplied.
"""

import ctypes
import errno
import json
import os
import platform
import stat
import tempfile
from pathlib import Path


RESOLVE_NO_XDEV = 0x01
RESOLVE_NO_SYMLINKS = 0x04
RESOLVE_BENEATH = 0x08


class UnsupportedCapability(RuntimeError):
    pass


class OpenHow(ctypes.Structure):
    _fields_ = [("flags", ctypes.c_uint64), ("mode", ctypes.c_uint64),
                ("resolve", ctypes.c_uint64)]


def require_linux():
    if platform.system() != "Linux" or platform.machine() not in ("x86_64", "aarch64"):
        raise UnsupportedCapability("probe requires Linux x86_64/aarch64; no fallback")


def acquire_metadata_fd(root_fd, relative_path):
    """Acquire a metadata-only handle beneath a root; reject all redirection.

    openat2 syscall 437 is restricted here to Linux x86_64/aarch64. The final
    component follows the same no-symlink policy as ancestors. This is not an
    authorization primitive for later pathname-based mutation.
    """
    require_linux()
    path = os.fsencode(relative_path)
    if not path or path.startswith(b"/") or b"\0" in path:
        raise ValueError("expected nonempty relative native path without NUL")
    how = OpenHow(os.O_PATH | os.O_CLOEXEC,
                  0, RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_XDEV)
    libc = ctypes.CDLL(None, use_errno=True)
    libc.syscall.restype = ctypes.c_long
    result = libc.syscall(ctypes.c_long(437), ctypes.c_int(root_fd),
                          ctypes.c_char_p(path), ctypes.byref(how),
                          ctypes.c_size_t(ctypes.sizeof(how)))
    if result < 0:
        code = ctypes.get_errno()
        if code in (errno.ENOSYS, errno.EINVAL, errno.E2BIG):
            raise UnsupportedCapability(f"openat2 unavailable: errno {code}; no fallback")
        raise OSError(code, os.strerror(code))
    return result


def observe(root_fd, relative_path):
    fd = acquire_metadata_fd(root_fd, relative_path)
    try:
        metadata = os.fstat(fd)
        return {
            "identity": [metadata.st_dev, metadata.st_ino],
            "apparent_bytes": metadata.st_size,
            "allocated_bytes": metadata.st_blocks * 512,
            "links": metadata.st_nlink,
            "regular_file": stat.S_ISREG(metadata.st_mode),
        }
    finally:
        os.close(fd)


def run_probe():
    require_linux()
    checks = {}
    with tempfile.TemporaryDirectory(prefix="omg-phase0-") as tmp:
        root = Path(tmp)
        (root / "nested").mkdir()
        # Writing synthetic fixtures is separate from metadata observation.
        (root / "nested" / "artifact").write_bytes(b"fixture")
        os.link(root / "nested" / "artifact", root / "hardlink")
        with (root / "sparse").open("wb") as stream:
            stream.truncate(8 * 1024 * 1024)
        (root / "link").symlink_to("nested", target_is_directory=True)
        (root / "cache").mkdir()
        (root / "cache" / ".git").mkdir()
        before = (root / "nested").stat().st_mtime_ns
        (root / "nested" / "artifact").write_bytes(b"larger fixture")
        after = (root / "nested").stat().st_mtime_ns
        checks["descendant_rewrite_keeps_parent_mtime"] = before == after
        root_fd = os.open(root, os.O_PATH | os.O_DIRECTORY | os.O_CLOEXEC)
        try:
            first = observe(root_fd, "nested/artifact")
            second = observe(root_fd, "hardlink")
            sparse = observe(root_fd, "sparse")
            checks["hardlink_identity_shared"] = first["identity"] == second["identity"]
            checks["hardlink_count_at_least_two"] = first["links"] >= 2
            checks["sparse_allocation_below_apparent"] = (
                sparse["allocated_bytes"] < sparse["apparent_bytes"])
            fd = acquire_metadata_fd(root_fd, "nested/artifact")
            try:
                try:
                    os.read(fd, 1)
                except OSError as error:
                    checks["metadata_handle_not_content_readable"] = error.errno == errno.EBADF
                else:
                    checks["metadata_handle_not_content_readable"] = False
            finally:
                os.close(fd)
            for name, path in (("ancestor_symlink_rejected", "link/artifact"),
                               ("final_symlink_rejected", "link"),
                               ("parent_escape_rejected", "../outside")):
                try:
                    fd = acquire_metadata_fd(root_fd, path)
                except OSError as error:
                    checks[name] = error.errno in (errno.ELOOP, errno.EXDEV)
                else:
                    os.close(fd)
                    checks[name] = False
            original = observe(root_fd, "nested/artifact")
            (root / "nested" / "artifact").rename(root / "original")
            (root / "nested" / "artifact").write_bytes(b"replacement")
            replacement = observe(root_fd, "nested/artifact")
            checks["replacement_identity_changes"] = original["identity"] != replacement["identity"]
            os.rename(root / "nested", root / "saved-parent")
            (root / "nested").symlink_to("saved-parent", target_is_directory=True)
            try:
                fd = acquire_metadata_fd(root_fd, "nested/artifact")
            except OSError as error:
                checks["substituted_parent_rejected"] = error.errno == errno.ELOOP
            else:
                os.close(fd)
                checks["substituted_parent_rejected"] = False
            # This is evidence of a protected-descendant fixture, not a policy engine.
            protected = observe(root_fd, "cache/.git")
            checks["protected_descendant_visible_in_metadata"] = not protected["regular_file"]
        finally:
            os.close(root_fd)
    return {
        "probe": "omg-phase0-metadata-v1",
        "environment": {"system": platform.system(), "machine": platform.machine(),
                        "kernel": platform.release()},
        "synthetic_fixtures_only": True,
        "checks": checks,
        "passed": all(checks.values()),
        "capability_denials": ["production_scanning", "cloud_nonhydration",
                               "mount_swap_certification", "trash_execution",
                               "windows_native", "macos_native"],
    }


if __name__ == "__main__":
    try:
        report = run_probe()
    except UnsupportedCapability as error:
        print(json.dumps({"passed": False, "unsupported": str(error)}))
        raise SystemExit(2)
    print(json.dumps(report, indent=2))
    raise SystemExit(0 if report["passed"] else 1)
