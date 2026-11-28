#!/usr/bin/env bash
# Builds the server as a normal user. Pinned dependencies:
# - crates via Cargo.lock (committed)
# - uring-kit/sqlite-btree/uring-http via crates.io version pins in Cargo.toml
# - SQLite amalgamation: vendored UNMODIFIED source at
#   crates/sqlite-ffi/csrc/sqlite3.c (compiled from source by build.rs,
#   rule 10; no prebuilt binaries)
set -euo pipefail
cd "$(dirname "$0")"

export PATH="/usr/local/bin:$HOME/.cargo/bin:$PATH"

# -O3 via the release profile; target-cpu=native (built on the box, rule 10)
export RUSTFLAGS="-C target-cpu=native"
cargo build --release --locked -p feed-server

test -x target/release/feed-server
echo "build ok"
