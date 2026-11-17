#!/bin/sh
set -eu
evidence=""
if test -n "${HITO_NATIVE_EVIDENCE:-}"; then
  evidence="$HITO_NATIVE_EVIDENCE/$1-$2-$(date -u +%Y%m%dT%H%M%SZ)-$$"
fi
docker compose --profile desktop run --rm \
  -e HITO_AUDIO_TEST_FEATURES="${HITO_AUDIO_TEST_FEATURES:-device}" \
  -e HITO_NATIVE_RUN="$evidence" \
  -e HITO_SOURCE_REVISION="$(git rev-parse --short=12 HEAD 2>/dev/null || echo unknown)" \
  desktop sh -c '
  set -eu
  package="$1"
  fixture="$2"
  test_root=$(mktemp -d)
  trap '\''rm -rf "$test_root"'\'' EXIT
  if ! cargo test --locked -p "$1" --features "$HITO_AUDIO_TEST_FEATURES" --test "$2" --no-run --message-format=json > "$test_root/build.json"; then
    python3 -c '\''import json,sys; [print(item["message"]["rendered"]) for line in open(sys.argv[1]) if (item:=json.loads(line)).get("reason")=="compiler-message"]'\'' "$test_root/build.json"
    exit 1
  fi
  test_binary=$(python3 -c '\''import json,sys; print(next(item["executable"] for line in open(sys.argv[1]) if (item:=json.loads(line)).get("executable") and item["target"]["name"]==sys.argv[2]))'\'' "$test_root/build.json" "$2")
  shift 2
  if test "${1:-}" = "--backend-fault-control"; then
    shift
    set -- --backend-fault-control "$test_binary" --ignored --test-threads=1 "$@"
  else
    set -- "$test_binary" --ignored --test-threads=1 "$@"
  fi
  if test -z "$HITO_NATIVE_RUN"; then
    exec sh scripts/desktop/with-audio.sh "$@"
  fi
  run="/workspace/$HITO_NATIVE_RUN"
  mkdir -p "$run/capture"
  chown 10001:10001 "$run/capture"
  export HITO_CAPTURE_DUMP="$run/capture"
  { if sh scripts/desktop/with-audio.sh "$@" 2>&1; then echo 0 > "$test_root/status"; else echo "$?" > "$test_root/status"; fi; } | tee "$run/test.log"
  status=$(cat "$test_root/status")
  reached=false
  if test -n "$(ls -A "$run/capture")"; then reached=true; fi
  printf "{\"package\":\"%s\",\"test\":\"%s\",\"revision\":\"%s\",\"exit_status\":%s,\"capture_reached\":%s}\n" \
    "$package" "$fixture" "$HITO_SOURCE_REVISION" "$status" "$reached" > "$run/run.json"
  if test "$status" = 0; then rm -rf "${run:?}/capture"; fi
  chmod -R a+rX "$run"
  exit "$status"
' sh "$@"
