#!/bin/sh
set -eu

hito_contract_dir=$(mktemp -d)
trap 'rm -rf "$hito_contract_dir"' EXIT

docker compose --profile tools run --rm rust cargo run --locked --quiet -p hito-server --example export_contract > "$hito_contract_dir/openapi.json"
if ! cmp -s docs/openapi.json "$hito_contract_dir/openapi.json"; then
  diff -u docs/openapi.json "$hito_contract_dir/openapi.json"
  exit 1
fi
cp apps/web/src/generated.ts "$hito_contract_dir/generated.ts"
docker compose --profile tools run --rm node npm run generate:api
if ! cmp -s apps/web/src/generated.ts "$hito_contract_dir/generated.ts"; then
  diff -u "$hito_contract_dir/generated.ts" apps/web/src/generated.ts
  exit 1
fi
