#!/usr/bin/env bash
# Kept for older notes: the ext4 sandbox is now `scripts/fs-sandbox.sh ext4 ...`.
exec "$(dirname "$0")/fs-sandbox.sh" ext4 "$@"
