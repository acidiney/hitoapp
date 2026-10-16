#!/bin/sh
set -eu
docker compose --profile desktop run --rm -e HITO_AUDIO_TEST_FEATURES="${HITO_AUDIO_TEST_FEATURES:-device}" desktop sh -c '
  set -eu
  test_root=$(mktemp -d)
  trap '\''rm -rf "$test_root"'\'' EXIT
  if ! cargo test --locked -p "$1" --features "$HITO_AUDIO_TEST_FEATURES" --test "$2" --no-run --message-format=json > "$test_root/build.json"; then
    python3 -c '\''import json,sys; [print(item["message"]["rendered"]) for line in open(sys.argv[1]) if (item:=json.loads(line)).get("reason")=="compiler-message"]'\'' "$test_root/build.json"
    exit 1
  fi
  test_binary=$(python3 -c '\''import json,sys; print(next(item["executable"] for line in open(sys.argv[1]) if (item:=json.loads(line)).get("executable") and item["target"]["name"]==sys.argv[2]))'\'' "$test_root/build.json" "$2")
  shift 2
  sh scripts/desktop/with-audio.sh "$test_binary" --ignored --test-threads=1 "$@"
' sh "$@"
