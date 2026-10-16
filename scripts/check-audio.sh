#!/bin/sh
set -eu

sh scripts/check-audio-model.sh
docker compose --profile tools run --rm rust cargo test --locked -p hito-audio
docker compose --profile desktop run --rm desktop cargo clippy --locked -p hito-audio --features device --all-targets -- -D warnings
docker compose --profile desktop run --rm desktop cargo test --locked -p hito-audio --features device --lib
sh scripts/run-audio-test.sh hito-audio device "$@"
sh scripts/run-audio-test.sh hito-audio tempo "$@"
HITO_AUDIO_TEST_FEATURES=device,native-probe sh scripts/run-audio-test.sh hito-audio native_ownership "$@"
