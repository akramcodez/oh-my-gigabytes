#!/usr/bin/env bash
# Simulates a kernel without openat2 (Linux older than 5.6) by making that system call return
# ENOSYS through a seccomp filter, and checks that omg refuses with a clear message instead of
# scanning without its protections (release plan R07). Needs a systemd user manager.
#
#   scripts/check-no-openat2.sh [path/to/omg]
set -euo pipefail

omg="$(realpath "${1:-target/debug/omg}")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir "$work/root"

"$omg" --state-dir "$work/normal-state" scan --root "$work/root" | grep -q '"state": "complete"' ||
  { echo "baseline scan failed" >&2; exit 1; }

# `|| true`: the filtered run is expected to fail; the message is what matters.
out="$(systemd-run --user --quiet --wait --pipe --collect \
  -p 'SystemCallFilter=~openat2' -p SystemCallErrorNumber=ENOSYS \
  "$omg" --state-dir "$work/filtered-state" scan --root "$work/root" 2>&1 || true)"
echo "$out"
grep -q 'openat2 is unavailable: omg needs Linux 5.6 or newer' <<< "$out" ||
  { echo "expected the Linux 5.6 message" >&2; exit 1; }
[ ! -e "$work/filtered-state/index.sqlite" ] || { echo "a database was created without openat2" >&2; exit 1; }
echo "ok: refused cleanly without openat2"
