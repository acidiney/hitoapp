#!/bin/sh
set -eu
docker compose --profile tools run --rm rust cargo test --locked -p hito-playback
docker compose --profile desktop run --rm desktop cargo clippy --locked -p hito-playback --features device --all-targets -- -D warnings
sh scripts/run-audio-test.sh hito-playback session "$@"
sh scripts/run-audio-test.sh hito-playback session_controls "$@"
sh scripts/run-audio-test.sh hito-playback session_logging "$@"
sh scripts/run-audio-test.sh hito-playback player_native "$@"
sh scripts/run-audio-test.sh hito-playback player_native_ownership "$@"
sh scripts/run-audio-test.sh hito-playback player_native_sources "$@"
sh scripts/run-audio-test.sh hito-playback player_native_completion "$@"
sh scripts/run-audio-test.sh hito-playback player_native_restore "$@"
sh scripts/run-audio-test.sh hito-playback player_logging "$@"
