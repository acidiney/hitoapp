#!/bin/sh
set -eu

docker compose --profile tools run --rm rust cargo fmt --all --check
docker compose --profile tools run --rm rust cargo test --workspace --locked
docker compose --profile tools run --rm -e 'RUSTFLAGS=--cfg rustix_use_libc' rust cargo test --locked -p hito-core --test storage_walk_stalls -- --ignored
docker compose --profile tools run --rm rust cargo clippy --workspace --all-targets --locked -- -D warnings
docker compose --profile tools run --rm node npm ci
./scripts/check-contracts.sh
docker compose --profile tools run --rm node npm test
docker compose --profile tools run --rm node npm run build
docker compose --profile tools run --rm node npm audit --audit-level=high
./scripts/check-setup.sh
docker compose --profile tools run --rm node node /workspace/scripts/audio-fixtures.mjs --e2e
docker compose --profile e2e rm -sf e2e-server
docker compose --profile e2e up --build -d e2e-server
trap 'docker compose --profile e2e stop e2e-server' EXIT
docker compose --profile e2e run --rm browser
