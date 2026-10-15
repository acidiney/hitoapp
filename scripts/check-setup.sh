#!/bin/sh
set -eu
umask 077
hito_setup_check=$(mktemp -d)
trap 'rm -rf "$hito_setup_check"' EXIT
docker compose --profile tools run --rm -w /workspace node node --test scripts/setup/prepare.test.mjs
docker run --rm -v "$PWD:/workspace:ro" -v "$hito_setup_check:/check" node:22-bookworm-slim node /workspace/scripts/setup/check-compose-paths.mjs prepare
HITO_SETUP_PATH_SENTINEL=expanded docker compose --project-directory "$hito_setup_check" -f "$hito_setup_check/compose.json" config --format json > "$hito_setup_check/resolved.json"
docker run --rm -v "$PWD:/workspace:ro" -v "$hito_setup_check:/check:ro" node:22-bookworm-slim node /workspace/scripts/setup/check-compose-paths.mjs verify
docker compose --profile tools run --rm node sh -n /workspace/scripts/setup/launch.sh
