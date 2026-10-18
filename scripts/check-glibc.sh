#!/usr/bin/env bash
# Fail if a Linux binary needs a newer glibc than allowed (release plan R08).
#   scripts/check-glibc.sh BINARY MAX_VERSION     e.g. target/release/omg 2.28
set -euo pipefail
binary="${1:?usage: check-glibc.sh BINARY MAX_VERSION}"
limit="${2:?usage: check-glibc.sh BINARY MAX_VERSION}"
needed="$(objdump -T "$binary" | grep -o 'GLIBC_[0-9.]*' | sed 's/^GLIBC_//' | sort -V | tail -1)"
echo "$binary needs glibc ${needed:-none} (limit $limit)"
newest="$(printf '%s\n%s\n' "${needed:-0}" "$limit" | sort -V | tail -1)"
[ "$newest" = "$limit" ] || { echo "error: requires glibc $needed, newer than $limit" >&2; exit 1; }
