#!/usr/bin/env bash
# Build the analysis-only omg binary inside a manylinux_2_28 container so it needs glibc 2.28 or
# newer instead of the build host's glibc (release plan R08).
#
#   scripts/build-manylinux.sh IMAGE OUT_DIR
#   IMAGE    quay.io/pypa/manylinux_2_28_x86_64 or quay.io/pypa/manylinux_2_28_aarch64
#   OUT_DIR  existing directory that receives `omg`
set -euo pipefail

image="${1:?usage: build-manylinux.sh IMAGE OUT_DIR}"
out="$(cd "${2:?usage: build-manylinux.sh IMAGE OUT_DIR}" && pwd)"
source="$(cd "$(dirname "$0")/.." && pwd)"

docker run --rm --user "$(id -u):$(id -g)" \
  -v "$source":/src:ro -v "$out":/out "$image" bash -ec '
    export HOME=/tmp/h CARGO_HOME=/tmp/h/.cargo RUSTUP_HOME=/tmp/h/.rustup
    mkdir -p "$HOME"
    curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs |
      sh -s -- -y --default-toolchain 1.95.0 --profile minimal --no-modify-path
    . "$CARGO_HOME/env"
    cd /src
    cargo build --release --locked -p omg-cli --no-default-features --target-dir /tmp/target
    cp /tmp/target/release/omg /out/omg
  '
