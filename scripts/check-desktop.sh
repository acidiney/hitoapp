#!/bin/sh
set -eu

docker compose --profile desktop build desktop
sh ./scripts/check-audio.sh
sh ./scripts/check-playback.sh
docker compose --profile tools run --rm node npm ci
docker compose --profile tools run --rm node npm run build
docker compose --profile tools run --rm node node /workspace/scripts/audio-fixtures.mjs --e2e
docker compose --profile desktop run --rm desktop cargo clippy --locked -p hito-desktop --features gui --all-targets -- -D warnings
docker compose --profile desktop run --rm desktop cargo test --locked -p hito-desktop --features gui
docker compose --profile desktop run --rm desktop cargo build --locked -p hito-desktop --features gui
docker compose --profile desktop run --rm desktop sh -c '
  mkdir -p /workspace/test-results/desktop
  chmod 700 /workspace/test-results/desktop
  chown 10001:10001 /workspace/test-results/desktop
  runuser -u hito-desktop -- xvfb-run -a --server-args="-screen 0 1600x1100x24" dbus-run-session -- node scripts/desktop/smoke.mjs
'
