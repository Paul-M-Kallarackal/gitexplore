#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C

artifact_dir=".gitexplore-release"
binary_path="$artifact_dir/gitexplore"
fingerprint_path="$artifact_dir/source.sha256"

cargo build --locked --release
install -Dm755 target/release/gitexplore "$binary_path"

find \
  Cargo.toml \
  Cargo.lock \
  rust-toolchain.toml \
  src \
  docker/neo4j/init \
  -type f -print0 \
  | sort -z \
  | xargs -0 sha256sum \
  | sha256sum \
  | cut -d ' ' -f 1 > "$fingerprint_path"
