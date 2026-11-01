#!/usr/bin/env bash
# Build and test omg natively on a Mac, with the same steps CI runs on its macOS legs
# (.github/workflows/check.yml). Installs nothing system-wide: Rust goes to ~/.cargo and ~/.rustup,
# Node to ~/.local. No sudo.
#
#   scripts/mac-check.sh                full CI-equivalent run
#   scripts/mac-check.sh --quick        fmt, clippy and analysis-only tests only
#   scripts/mac-check.sh --smoke        also run the built omg against real paths (private state dir)
#   scripts/mac-check.sh --no-bootstrap fail instead of installing Rust or Node
set -euo pipefail

quick=0 smoke=0 bootstrap=1
for arg in "$@"; do
  case "$arg" in
    --quick) quick=1 ;;
    --smoke) smoke=1 ;;
    --no-bootstrap) bootstrap=0 ;;
    -h | --help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

[ "$(uname -s)" = Darwin ] || { echo "mac-check.sh runs on macOS only" >&2; exit 2; }
darwin=$(uname -r | cut -d. -f1)
# omg's own floor: an older kernel could ignore the open flags it relies on (docs/macos-evidence.md).
[ "$darwin" -ge 24 ] || { echo "Darwin $darwin is older than 24 (macOS 15); omg refuses to scan here" >&2; exit 2; }
command -v cc > /dev/null || { echo "Xcode Command Line Tools missing: run 'xcode-select --install'" >&2; exit 2; }

cd "$(dirname "$0")/.."
repo=$(pwd -P)

# omg refuses links in a scan root or state path, and macOS temp folders sit under /var (a link).
TMPDIR=$(cd "${TMPDIR:-/private/tmp}" && pwd -P)
export TMPDIR
# A runner that cannot mount disk images is a failure here, not a skip, as in CI.
export OMG_REQUIRE_DISK_IMAGES=1
export CARGO_TERM_COLOR=always

say() { printf '\n==> %s\n' "$*"; }

# ---- bootstrap ---------------------------------------------------------------------------------
export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v rustup > /dev/null; then
  [ "$bootstrap" = 1 ] || { echo "rustup missing (re-run without --no-bootstrap)" >&2; exit 2; }
  say "Installing rustup (user-level; the pinned toolchain comes from rust-toolchain.toml)"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none --no-modify-path
fi
# Running cargo inside the repo installs the toolchain pinned in rust-toolchain.toml if needed.
say "Rust toolchain"
cargo --version
rustc --version

node_major() { command -v node > /dev/null && node --version | sed 's/^v//; s/\..*//' || echo 0; }
if [ "$(node_major)" -lt 22 ]; then
  local_node=$(ls -d "$HOME"/.local/node-v22.*/bin 2> /dev/null | sort -V | tail -1 || true)
  if [ -z "$local_node" ]; then
    [ "$bootstrap" = 1 ] || { echo "Node 22+ missing (re-run without --no-bootstrap)" >&2; exit 2; }
    say "Installing Node 22 into ~/.local (checksum verified)"
    case "$(uname -m)" in arm64) arch=arm64 ;; x86_64) arch=x64 ;; *) echo "unsupported CPU $(uname -m)" >&2; exit 2 ;; esac
    base=https://nodejs.org/dist/latest-v22.x
    work=$(mktemp -d "$TMPDIR/omg-node.XXXXXX")
    curl -sSfL "$base/SHASUMS256.txt" -o "$work/SHASUMS256.txt"
    name=$(grep -o "node-v22[0-9.]*-darwin-$arch\.tar\.gz" "$work/SHASUMS256.txt" | head -1)
    [ -n "$name" ] || { echo "no darwin-$arch Node 22 tarball listed" >&2; exit 1; }
    curl -sSfL "$base/$name" -o "$work/$name"
    (cd "$work" && grep " $name\$" SHASUMS256.txt | shasum -a 256 -c -)
    mkdir -p "$HOME/.local"
    tar -xzf "$work/$name" -C "$HOME/.local"
    rm -rf "$work"
    local_node="$HOME/.local/${name%.tar.gz}/bin"
  fi
  export PATH="$local_node:$PATH"
fi
say "Node $(node --version), Python $(python3 --version | cut -d' ' -f2), macOS $(sw_vers -productVersion) $(uname -m)"
echo "TMPDIR=$TMPDIR"

# ---- steps (mirrors check.yml; order matters, see the no-default-features note below) ------------
results=()
failed=0
step() {
  local name=$1
  shift
  say "$name"
  if "$@"; then
    results+=("PASS  $name")
  else
    results+=("FAIL  $name")
    failed=1
  fi
}

cleanup_images() {
  # Detach any disk image an aborted test left mounted (they are named omg-*).
  hdiutil info 2> /dev/null | awk '/^image-path/ && /omg-/ {found=1} found && /^\/dev\/disk[0-9]+ / {print $1; found=0}' |
    while read -r dev; do hdiutil detach "$dev" -force > /dev/null 2>&1 || true; done
}
trap cleanup_images EXIT

step "cargo fmt --check" cargo fmt --all -- --check
if [ "$quick" = 0 ]; then
  step "clippy (default build)" cargo clippy --workspace --all-targets --locked -- -D warnings
fi
step "clippy (analysis-only build)" cargo clippy --workspace --all-targets --no-default-features --locked -- -D warnings
if [ "$quick" = 0 ]; then
  step "tests (default build)" cargo test --workspace --locked --no-fail-fast
fi
step "tests (analysis-only build)" cargo test --workspace --no-default-features --locked --no-fail-fast
if [ "$quick" = 0 ] || [ "$smoke" = 1 ]; then
  # The analysis-only test run above replaced target/debug/omg; rebuild the default binary before
  # anything that runs it.
  step "build omg (default)" cargo build -p omg-cli --locked
fi
if [ "$quick" = 0 ]; then
  step "terminal smoke test (PTY)" python3 -I -m unittest discover -s tests -p 'test_*.py' -v
  step "npm launcher tests" node --test packages/oh-my-gigabytes/test/release.test.js
fi

# ---- optional: the built binary against real paths -----------------------------------------------
smoke_run() {
  local omg="$repo/target/debug/omg"
  local base state tree
  base=$(mktemp -d "$TMPDIR/omg-smoke.XXXXXX")
  state="$base/state"
  tree="$base/tree"
  mkdir -m 700 "$state"
  mkdir -p "$tree/project/node_modules/pkg" "$tree/project/src"
  head -c 1048576 /dev/zero > "$tree/project/node_modules/pkg/blob.bin"
  echo 'fn main() {}' > "$tree/project/src/main.rs"
  ln -s /etc "$tree/link-to-etc"
  local rc=0
  o() { echo "+ omg $*"; "$omg" --state-dir "$state" "$@"; }
  o doctor || rc=1
  o scan --root "$tree" || rc=1
  o status || rc=1
  o tree || rc=1
  o findings || rc=1
  # Roots omg must refuse: the sealed system volume, and a link (/tmp -> /private/tmp).
  for refused in / /tmp; do
    if o scan --root "$refused"; then
      echo "expected omg to refuse $refused" >&2
      rc=1
    else
      echo "(refused $refused as expected)"
    fi
  done
  # A real home folder: may be partial where macOS privacy protection (TCC) denies a folder.
  echo "+ omg scan --root $HOME (60 s cap)"
  perl -e 'alarm 60; exec @ARGV' "$omg" --state-dir "$state" scan --root "$HOME" || echo "(home scan exited non-zero: $?; see output above)"
  o status || rc=1
  rm -rf "$base"
  return $rc
}
if [ "$smoke" = 1 ]; then
  step "smoke: built omg on real paths" smoke_run
fi

say "Summary"
printf '%s\n' "${results[@]}"
exit "$failed"
