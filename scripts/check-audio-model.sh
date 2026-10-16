#!/bin/sh
set -eu
docker compose --profile tools run --rm \
  -e CARGO_TARGET_DIR=/build/loom-target \
  -e 'RUSTFLAGS=--cfg loom' \
  rust cargo test --locked -p hito-audio --lib state_model -- --test-threads=1
