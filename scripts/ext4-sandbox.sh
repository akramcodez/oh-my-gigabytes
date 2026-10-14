#!/usr/bin/env bash
# Disposable ext4 sandbox for Linux qualification tests (WP05, WP12).
# Creates a small image file, formats it ext4, and mounts it at MOUNT.
# Only the mount step needs root. Everything else runs as the current user.
#
#   scripts/ext4-sandbox.sh up      # create + mount (sudo for mkfs/mount/chown)
#   scripts/ext4-sandbox.sh test    # run the cleanup suite with TMPDIR on ext4
#   scripts/ext4-sandbox.sh down    # unmount and delete the image
set -euo pipefail

IMAGE="${OMG_EXT4_IMAGE:-/var/tmp/omg-ext4-sandbox.img}"
MOUNT="${OMG_EXT4_MOUNT:-/mnt/omg-ext4-sandbox}"
SIZE="${OMG_EXT4_SIZE:-512M}"

case "${1:-}" in
  up)
    if mountpoint -q "$MOUNT"; then
      echo "already mounted at $MOUNT"; exit 0
    fi
    truncate -s "$SIZE" "$IMAGE"
    sudo mkfs.ext4 -q -F "$IMAGE"
    sudo mkdir -p "$MOUNT"
    sudo mount -o loop,nosuid,nodev "$IMAGE" "$MOUNT"
    sudo chown "$(id -u):$(id -g)" "$MOUNT"
    chmod 700 "$MOUNT"
    echo "ext4 sandbox ready at $MOUNT ($(stat -f -c %T "$MOUNT"))"
    probe="$MOUNT/birth-probe"; echo probe > "$probe"
    echo "birth time reported: $(stat -c %w "$probe" | grep -qv '^-$' && echo yes || echo no)"
    rm -f "$probe"
    ;;
  test)
    fstype="$(stat -f -c %T "$MOUNT" 2>/dev/null || true)"
    if [ "$fstype" != "ext2/ext3" ] && [ "$fstype" != "ext4" ]; then
      echo "refusing: $MOUNT is '$fstype', not ext4. Run '$0 up' first." >&2; exit 1
    fi
    cd "$(dirname "$0")/.."
    TMPDIR="$MOUNT" cargo test -p omg-core --test cleanup --locked
    TMPDIR="$MOUNT" cargo test -p omg-core --test scanner --locked
    ;;
  down)
    if mountpoint -q "$MOUNT"; then sudo umount "$MOUNT"; fi
    rm -f "$IMAGE"
    echo "ext4 sandbox removed"
    ;;
  *)
    echo "usage: $0 up|test|down" >&2; exit 2 ;;
esac
