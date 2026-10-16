#!/bin/sh
set -eu
docker compose --profile tools run --rm rust cargo test --locked -p hito-playback
docker compose --profile desktop run --rm desktop cargo clippy --locked -p hito-playback --features device --all-targets -- -D warnings
sh scripts/run-audio-test.sh hito-playback session "$@"
sh scripts/run-audio-test.sh hito-playback session_controls "$@"
sh scripts/run-audio-test.sh hito-playback session_logging "$@"
