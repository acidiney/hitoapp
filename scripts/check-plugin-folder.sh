#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
check() {
  docker compose -p hito-plugin-folder -f compose.yaml -f scripts/plugins/compose.tools.yaml --profile tools run --rm --no-deps rust "$@"
}
check cargo fmt --all -- --check
check cargo test --locked -j 2 -p hito-plugin-sdk -p hito-plugin-folder
check cargo clippy --locked -j 2 -p hito-plugin-folder --all-targets -- -D warnings
