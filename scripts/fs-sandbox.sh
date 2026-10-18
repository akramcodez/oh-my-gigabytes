#!/usr/bin/env bash
# Disposable filesystem sandbox for Linux qualification tests (WP05, WP12, R07).
# Creates a small image file, formats it with the chosen filesystem and mounts it at MOUNT.
# Only up and down need root. Everything else runs as the current user.
#
#   scripts/fs-sandbox.sh ext4|xfs|btrfs up     # create + mount (sudo for mkfs/mount/chown)
#   scripts/fs-sandbox.sh ext4|xfs|btrfs test   # run the suites with TMPDIR on that filesystem
#   scripts/fs-sandbox.sh ext4|xfs|btrfs down   # unmount and delete the image
#
# Needs mkfs.ext4, mkfs.xfs (xfsprogs) or mkfs.btrfs (btrfs-progs) for the chosen filesystem.
# XFS is scanned but permanent delete is refused there, so its test run skips the cleanup suite.
set -euo pipefail

FS="${1:-}"
ACTION="${2:-}"
case "$FS" in
  ext4)  MKFS=(mkfs.ext4 -q -F); EXPECT="ext2/ext3 ext4" ;;
  xfs)   MKFS=(mkfs.xfs -q -f);  EXPECT="xfs" ;;
  btrfs) MKFS=(mkfs.btrfs -q -f); EXPECT="btrfs" ;;
  *) echo "usage: $0 ext4|xfs|btrfs up|test|down" >&2; exit 2 ;;
esac

IMAGE="${OMG_SANDBOX_IMAGE:-/var/tmp/omg-$FS-sandbox.img}"
MOUNT="${OMG_SANDBOX_MOUNT:-/mnt/omg-$FS-sandbox}"
SIZE="${OMG_SANDBOX_SIZE:-512M}"

fstype() { stat -f -c %T "$MOUNT" 2>/dev/null || true; }

case "$ACTION" in
  up)
    if mountpoint -q "$MOUNT"; then
      echo "already mounted at $MOUNT"; exit 0
    fi
    command -v "${MKFS[0]}" >/dev/null || { echo "missing ${MKFS[0]}; install the matching package" >&2; exit 1; }
    truncate -s "$SIZE" "$IMAGE"
    sudo "${MKFS[@]}" "$IMAGE"
    sudo mkdir -p "$MOUNT"
    sudo mount -o loop,nosuid,nodev "$IMAGE" "$MOUNT"
    sudo chown "$(id -u):$(id -g)" "$MOUNT"
    chmod 700 "$MOUNT"
    echo "$FS sandbox ready at $MOUNT ($(fstype))"
    probe="$MOUNT/birth-probe"; echo probe > "$probe"
    echo "birth time reported: $(stat -c %w "$probe" | grep -qv '^-$' && echo yes || echo no)"
    rm -f "$probe"
    ;;
  test)
    actual="$(fstype)"
    case " $EXPECT " in
      *" $actual "*) ;;
      *) echo "refusing: $MOUNT is '$actual', not $FS. Run '$0 $FS up' first." >&2; exit 1 ;;
    esac
    cd "$(dirname "$0")/.."
    TMPDIR="$MOUNT" cargo test -p omg-core --test scanner --locked
    TMPDIR="$MOUNT" cargo test -p omg-core --test rollup --locked
    TMPDIR="$MOUNT" cargo test -p omg-core --test policy --locked
    if [ "$FS" != xfs ]; then
      TMPDIR="$MOUNT" cargo test -p omg-core --test cleanup --locked
    fi
    if [ "$FS" = btrfs ]; then
      OMG_BTRFS_TEST_DIR="$MOUNT" TMPDIR="$MOUNT" cargo test -p omg-core --test scanner --locked -- --ignored
    fi
    ;;
  down)
    if mountpoint -q "$MOUNT"; then sudo umount "$MOUNT"; fi
    rm -f "$IMAGE"
    echo "$FS sandbox removed"
    ;;
  *)
    echo "usage: $0 ext4|xfs|btrfs up|test|down" >&2; exit 2 ;;
esac
