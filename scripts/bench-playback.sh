#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
export BENCH_RUN_ID="$(date -u +%Y%m%d%H%M%S)-$$"
started_seconds=$(date +%s)
fail() {
  printf '{"event":"benchmark_runner","run_id":"%s","outcome":"failed","code":"%s","elapsed_ms":%s}\n' "$BENCH_RUN_ID" "$1" "$((($(date +%s) - started_seconds) * 1000))" >&2
  exit 1
}

case "${1:-}" in
  --help)
    printf "%s\n" "Usage: scripts/bench-playback.sh [--help]"       "No server/tools image is built or pulled by this runner."       "Freeze source, then get its exact fingerprint: node scripts/bench/resource-guard.mjs identity"       "Prepare a prebuilt server image with a separately qualified guarded Release build from that fingerprint."       "Required labels: org.hito.benchmark.source_id=<fingerprint> and org.hito.benchmark.server.sha256=<qualified binary SHA256>."       "Set BENCH_SERVER_IMAGE=<immutable image ID/digest>, BENCH_SERVER_SHA256=<qualified binary SHA256>, and BENCH_NATIVE_TOOLS_IMAGE=<qualified immutable desktop-tools image ID/digest>."       "The runner checks source fingerprint, image labels and actual server binary bytes. Labels alone do not qualify a build."       "Native compilation uses the shipped guard, the shared primary target lock, all Hito target volumes, registered worktree targets and inactive BuildKit Cargo caches."       "BENCH_RESULTS selects a fresh output directory; BENCH_PROJECT must start with hito-bench-."       "BENCH_STEPS defaults to library browser native report. Native collection always runs the native acceptance gate."       "Report-only use needs no server/tools image. This runner does not establish reference hardware or a 900-attempt result."
    exit 0 ;;
  '') ;;
  *) fail argument_invalid ;;
esac
provided_source=${BENCH_SOURCE_ID:-}
export COMPOSE_FILE="compose.yaml:scripts/bench/compose.yaml"
export BENCH_SAMPLES="${BENCH_SAMPLES:-100}"
export BENCH_RTT_MS="${BENCH_RTT_MS:-40}"
results="${BENCH_RESULTS:-test-results/latency}"
steps="${BENCH_STEPS:-library browser native report}"
mkdir -p "$results"
export BENCH_RESULTS_ABSOLUTE="$(cd "$results" && pwd -P)"
results=$BENCH_RESULTS_ABSOLUTE
identity=$(node scripts/bench/resource-guard.mjs identity) || exit 1
export BENCH_SOURCE_ID=${identity%% *}
export BENCH_TARGET_NAMESPACE=${identity#* }
case "$BENCH_SOURCE_ID" in ''|*[!a-f0-9]*) fail source_identity_invalid ;; esac
[ "${#BENCH_SOURCE_ID}" = 64 ] || fail source_identity_invalid
[ -z "$provided_source" ] || [ "$provided_source" = "$BENCH_SOURCE_ID" ] || fail source_identity_mismatch
export COMPOSE_PROJECT_NAME="${BENCH_PROJECT:-hito-bench-${BENCH_SOURCE_ID}-${BENCH_RUN_ID}}"
case "$COMPOSE_PROJECT_NAME" in hito-bench-*) ;; *) fail project_not_owned ;; esac
case "$COMPOSE_PROJECT_NAME" in *[!a-z0-9_-]*) fail project_not_owned ;; esac
[ "${#COMPOSE_PROJECT_NAME}" -le 120 ] || fail project_not_owned
server_started=0
server_owned=0
native_owned=0
native_client=
build_client=
lock_owned=0
lock_directory="${TMPDIR:-/tmp}/$COMPOSE_PROJECT_NAME.lock"
native_required=0
report_ran=0
collecting=0
for step in $steps; do
  case "$step" in
    library) collecting=1 ;;
    browser) collecting=1 ;;
    native) collecting=1; native_required=1 ;;
    report) ;;
    *) fail step_invalid ;;
  esac
done

record_host() {
  {
    echo "date_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "commit: $(git rev-parse HEAD)"
    echo "source_id: $BENCH_SOURCE_ID"
    if [ "$(uname -s)" = Darwin ]; then
      echo "host_model: $(sysctl -n hw.model)"
      echo "host_cpu: $(sysctl -n machdep.cpu.brand_string)"
      echo "host_logical_cpus: $(sysctl -n hw.ncpu)"
      echo "host_performance_cpus: $(sysctl -n hw.perflevel0.logicalcpu 2>/dev/null || echo unknown)"
      echo "host_efficiency_cpus: $(sysctl -n hw.perflevel1.logicalcpu 2>/dev/null || echo unknown)"
      echo "host_memory_bytes: $(sysctl -n hw.memsize)"
      echo "host_os: $(sw_vers -productName) $(sw_vers -productVersion)"
    fi
    docker info --format 'docker_server: {{.ServerVersion}} {{.OperatingSystem}}
docker_kernel: {{.KernelVersion}}
docker_cpus: {{.NCPU}}
docker_memory_bytes: {{.MemTotal}}'
    echo "network: loopback inside one container network namespace; provider latency is injected per request"
  } > "$results/host.txt"
}

require_idle() {
  busy=$(docker ps -a --filter "label=com.docker.compose.project=$COMPOSE_PROJECT_NAME" --format "{{.ID}}" 2>/dev/null) || fail docker_inspection_failed
  [ -z "$busy" ] || fail project_busy
}


require_empty_outputs() {
  for file in "$results"/"$1"-*; do
    if [ -e "$file" ] || [ -L "$file" ]; then fail "${1}_output_exists"; fi
  done
}

step_library() {
  node scripts/bench/resource-guard.mjs verify-tools || return $?
  docker compose --profile tools run --rm --no-deps --pull never -T bench-library
}

step_browser() {
  require_empty_outputs browser
  node scripts/bench/resource-guard.mjs verify-server || return $?
  server_owned=1
  docker compose --profile e2e up --no-build --pull never -d --wait e2e-server || return $?
  server_started=1
  docker compose --profile e2e run --rm --no-deps --pull never -T browser
}

step_native() {
  require_empty_outputs native
  export BENCH_NATIVE_STAGING="$(mktemp -d "$results/.native-staging.XXXXXX")"
  chmod 777 "$BENCH_NATIVE_STAGING"
  node scripts/bench/resource-guard.mjs native-build &
  build_client=$!
  build_status=0
  wait "$build_client" || build_status=$?
  build_client=
  [ "$build_status" = 0 ] || return "$build_status"
  native_owned=1
  node scripts/bench/resource-guard.mjs native-run sh -c '
    set -eu
    destination=$1
    staging=/benchmark-native-staging
    retained=$staging
    publish() {
      status=$?
      trap - EXIT HUP INT TERM
      export_failed=0
      for file in "$staging"/native-*; do
        [ -f "$file" ] || continue
        name=${file##*/}
        (set -C; cat "$file" > "$destination/$name") 2>/dev/null || export_failed=1
      done
      printf "{\"exit\":%s}\n" "$status" > "$retained/native-execution.json"
      (set -C; cat "$retained/native-execution.json" > "$destination/native-execution.json") 2>/dev/null || export_failed=1
      if [ "$export_failed" != 0 ]; then
        printf "{\"event\":\"benchmark_runner\",\"outcome\":\"failed\",\"code\":\"native_export_failed\"}\n" >&2
        exit 1
      fi
      exit "$status"
    }
    trap publish EXIT
    trap "exit 129" HUP
    trap "exit 130" INT
    trap "exit 143" TERM
    binary="$CARGO_TARGET_DIR/release/examples/start_latency"
    BENCH_OUT="$staging" sh scripts/desktop/with-audio.sh "$binary"
  ' bench-native /benchmark-results &
  native_client=$!
  native_status=0
  wait "$native_client" || native_status=$?
  native_client=
  return "$native_status"
}

step_report() {
  report_ran=1
  native_flag=
  if [ "$native_required" = 1 ]; then native_flag=--gate-native; fi
  for file in "$results"/native-*; do
    if [ -e "$file" ] || [ -L "$file" ]; then native_flag=--gate-native; break; fi
  done
  report_status=0
  docker compose --profile tools run --rm --no-deps --pull never -T node sh -c '
    cd /workspace
    status=0
    node scripts/bench/report.mjs "$1" "$2" || status=$?
    node scripts/bench/chart.mjs "$1" || status=$?
    exit "$status"
  ' bench-report /benchmark-results "$native_flag" || report_status=$?
  if [ "$server_started" = 1 ]; then
    docker compose --profile e2e run --rm --no-deps --pull never -T -e BENCH_CHART=1 browser npx playwright test -c playwright.latency.config.ts || report_status=$?
  fi
  return "$report_status"
}

cleanup() {
  status=$?
  trap - EXIT HUP INT TERM
  if [ -n "$build_client" ]; then
    kill -TERM "$build_client" 2>/dev/null || true
    wait "$build_client" 2>/dev/null || true
  fi
  if [ -n "$native_client" ]; then
    kill -TERM "$native_client" 2>/dev/null || true
    wait "$native_client" 2>/dev/null || true
  fi
  if [ "$native_owned" = 1 ]; then
    node scripts/bench/resource-guard.mjs retire-native || status=1
  fi
  if [ "$server_owned" = 1 ]; then
    node scripts/bench/resource-guard.mjs retire-server || status=1
  fi
  networks=$(docker network ls --filter "label=com.docker.compose.project=$COMPOSE_PROJECT_NAME" --filter "label=org.hito.benchmark.run=$BENCH_RUN_ID" --format "{{.ID}}" 2>/dev/null) || networks=
  for network in $networks; do docker network rm "$network" >/dev/null 2>&1 || true; done
  if [ "$lock_owned" = 1 ]; then rmdir "$lock_directory" 2>/dev/null || true; fi
  exit "$status"
}

for step in $steps; do
  case "$step" in native|browser) require_empty_outputs "$step" ;; esac
done
if [ "$collecting" = 1 ] && { [ -e "$results/host.txt" ] || [ -L "$results/host.txt" ]; }; then fail host_output_exists; fi
mkdir "$lock_directory" 2>/dev/null || fail project_busy
lock_owned=1
trap cleanup EXIT
trap "exit 129" HUP
trap "exit 130" INT
trap "exit 143" TERM
require_idle
if [ "$collecting" = 1 ]; then record_host; fi
runner_status=0
for step in $steps; do
  if [ "$runner_status" != 0 ] && [ "$step" != report ]; then continue; fi
  step_status=0
  "step_$step" || step_status=$?
  if [ "$runner_status" = 0 ]; then runner_status=$step_status; fi
done
if [ "$report_ran" = 0 ]; then
  for file in "$results"/native-*; do
    if [ -e "$file" ] || [ -L "$file" ]; then native_required=1; break; fi
  done
  if [ "$native_required" = 1 ]; then
    report_status=0
    step_report || report_status=$?
    if [ "$runner_status" = 0 ]; then runner_status=$report_status; fi
  fi
fi
[ "$runner_status" = 0 ] || exit "$runner_status"
printf '{"event":"benchmark_runner","run_id":"%s","outcome":"complete","elapsed_ms":%s}\n' "$BENCH_RUN_ID" "$((($(date +%s) - started_seconds) * 1000))"
