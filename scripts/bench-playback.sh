#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
export COMPOSE_PROJECT_NAME="${BENCH_PROJECT:-hito-arch-latency}"
export COMPOSE_FILE="compose.yaml:scripts/bench/compose.yaml"
export BENCH_SERVER_CACHE_ID="${BENCH_SERVER_CACHE_ID:-hito-arch-latency-release}"
export BENCH_SAMPLES="${BENCH_SAMPLES:-100}"
export BENCH_RTT_MS="${BENCH_RTT_MS:-40}"
results="${BENCH_RESULTS:-test-results/latency}"
steps="${BENCH_STEPS:-library browser native report}"

mkdir -p "$results"

record_host() {
  {
    echo "date_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "commit: $(git rev-parse HEAD)"
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
  busy=$(docker ps --format '{{.Names}}' | grep -c '^hito-arch-latency' || true)
  if [ "$busy" != 0 ]; then
    echo "Another hito-arch-latency container is running; stop it first." >&2
    exit 1
  fi
}

step_library() {
  docker compose --profile bench run --rm -T bench-library
}

step_browser() {
  rm -f "$results"/browser-*
  docker compose --profile e2e up --build -d --wait e2e-server
  docker compose --profile e2e run --rm -T browser
}

step_native() {
  rm -f "$results"/native-*
  docker compose --profile desktop run --rm -T desktop sh -c '
    set -eu
    cargo build --release --locked -p hito-playback --features device --example start_latency
    binary="$CARGO_TARGET_DIR/release/examples/start_latency"
    staging=$(mktemp -d)
    chmod 777 "$staging"
    BENCH_OUT="$staging" sh scripts/desktop/with-audio.sh "$binary"
    mkdir -p /workspace/test-results/latency
    cp "$staging"/native-* /workspace/test-results/latency/
  '
}

step_report() {
  docker compose --profile tools run --rm -T node sh -c '
    cd /workspace
    node scripts/bench/report.mjs test-results/latency
    node scripts/bench/chart.mjs test-results/latency
  '
  docker compose --profile e2e run --rm -T -e BENCH_CHART=1 browser npx playwright test -c playwright.latency.config.ts
}

cleanup() {
  docker compose --profile e2e --profile desktop --profile tools --profile bench rm -sf >/dev/null 2>&1 || true
  docker compose --profile e2e down --remove-orphans >/dev/null 2>&1 || true
}
trap cleanup EXIT

require_idle
record_host
for step in $steps; do
  "step_$step"
done
echo "Results are in $results"
