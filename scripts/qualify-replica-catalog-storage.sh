#!/bin/sh
set -eu
umask 077
cd "$(dirname "$0")/.."
checkout=$(pwd -P)
offline=0
case "${1:-}" in
  '') ;;
  --offline) offline=1 ;;
  --help)
    echo "Usage: scripts/qualify-replica-catalog-storage.sh [--offline]"
    echo "Default: build rust-tools, fetch locked dependencies online, then qualify offline."
    echo "--offline skips dependency fetching and requires an already populated cache."
    echo "Set HITO_CATALOG_STORAGE_TOOLS_IMAGE to use an existing local tools image."
    echo "Optional volumes: HITO_CATALOG_STORAGE_TARGET_VOLUME, HITO_CATALOG_STORAGE_REGISTRY_VOLUME, HITO_CATALOG_STORAGE_GIT_VOLUME."
    echo "Set HITO_CATALOG_STORAGE_EXTERNAL_CACHES=true to require existing cache volumes."
    echo "Optional lock: HITO_CATALOG_STORAGE_LOCK, default /build/.hito-cargo-quality.lock."
    echo "The guard measures named Hito target volumes, the selected volume and sibling hito*/target directories."
    echo "Inspect custom targets and BuildKit compile caches separately before running."
    exit 0 ;;
  *) echo "Unknown option; use --help." >&2; exit 2 ;;
esac
[ "$#" -le 1 ] || { echo "Too many arguments." >&2; exit 2; }

mkdir -p test-results/replica-catalog-storage
results=$(mktemp -d test-results/replica-catalog-storage/run.XXXXXX)
printf 'phase\texit\tpassed\tfailed\tignored\tmeasured\tfiltered\n' > "$results/results.tsv"
current_phase=setup
failure_recorded=0
trap 'status=$?; if [ "$status" -ne 0 ] && [ "$failure_recorded" = 0 ]; then printf "%s\t%s\t-\t-\t-\t-\t-\n" "$current_phase" "$status" >> "$results/results.tsv"; fi' 0
entry=crates/core/tests/replica_catalog_storage/main.rs
[ -f "$entry" ] || { echo "Catalog storage qualification test target is missing." >&2; exit 1; }
git rev-parse HEAD > "$results/revision.tmp" 2>> "$results/setup.log"
git ls-files --cached --others --exclude-standard > "$results/source-paths.tmp" 2>> "$results/setup.log"
git hash-object --stdin-paths --no-filters < "$results/source-paths.tmp" > "$results/source-hashes.tmp" 2>> "$results/setup.log"
printf '%s\n' "$checkout" > "$results/fingerprint.tmp"
cat "$results/revision.tmp" "$results/source-paths.tmp" "$results/source-hashes.tmp" >> "$results/fingerprint.tmp"
shasum -a 256 "$results/fingerprint.tmp" > "$results/source-id.tmp" 2>> "$results/setup.log"
source_id=$(cut -c 1-32 "$results/source-id.tmp")
rm "$results/revision.tmp" "$results/source-paths.tmp" "$results/source-hashes.tmp" "$results/fingerprint.tmp" "$results/source-id.tmp"
export HITO_CATALOG_STORAGE_SOURCE_ID="replica-catalog-storage-$source_id"
printf '%s\n' "$HITO_CATALOG_STORAGE_SOURCE_ID" > "$results/source-id.txt"
lock=${HITO_CATALOG_STORAGE_LOCK:-/build/.hito-cargo-quality.lock}
case "$lock" in /build/*) ;; *) echo "The Cargo lock must be inside /build." >&2; exit 2;; esac

compose() {
  docker compose -p hito-replica-catalog-storage -f compose.yaml -f scripts/replicas/storage.tools.yaml --profile tools "$@"
}

check_host_space() {
  current_phase=resources
  host_space=$(df -Pk "$checkout" 2>> "$results/resource.log")
  available=$(printf '%s\n' "$host_space" | awk 'END {print $4}')
  [ "$available" -ge 41943040 ] || { echo "Stopped: host free space is below 40 GiB." >&2; exit 1; }
}

check_resources() {
  check_host_space
  docker volume ls --format '{{.Name}}' > "$results/volumes.tmp"
  awk -v selected="${HITO_CATALOG_STORAGE_TARGET_VOLUME:-hito-replica-catalog-storage-target}" '
    (tolower($0) ~ /hito/ && tolower($0) ~ /target/) || $0 == selected
  ' "$results/volumes.tmp" > "$results/targets.tmp"
  discovered_volumes=$(awk 'END {print NR}' "$results/targets.tmp")
  measured_volumes=0
  docker_kib=0
  while IFS= read -r volume; do
    export HITO_CATALOG_STORAGE_SIZE_VOLUME="$volume"
    if compose run --rm --no-deps --pull never --interactive=false -T storage-size < /dev/null > "$results/size.tmp" 2> "$results/resource.log"; then :; else
      resource_status=$?
      echo "Failed to inspect a Hito Cargo target volume." >&2; exit "$resource_status"
    fi
    size=$(awk 'NF == 2 && $1 ~ /^[0-9]+$/ && $2 == "/target" {count++; size=$1} END {if(count != 1) exit 1; print size}' "$results/size.tmp")
    docker_kib=$((docker_kib + size))
    measured_volumes=$((measured_volumes + 1))
  done < "$results/targets.tmp"
  [ "$measured_volumes" -eq "$discovered_volumes" ] || { echo "Stopped: resource volume measurement count differs from discovery." >&2; exit 1; }
  rm -f "$results/volumes.tmp" "$results/targets.tmp" "$results/size.tmp"
  host_kib=0
  for tree in "$checkout"/../hito*; do
    if [ -d "$tree/target" ]; then
      host_size=$(du -sk "$tree/target" 2>> "$results/resource.log")
      size=$(printf '%s\n' "$host_size" | awk '{print $1}')
      host_kib=$((host_kib + size))
    fi
  done
  total=$(((docker_kib + host_kib) * 1024))
  [ "$total" -lt 10737418240 ] || { echo "Stopped: combined Hito Cargo targets reached 10 GiB." >&2; exit 1; }
  printf 'resources: host_free_kib=%s cargo_target_bytes=%s target_volumes=%s\n' "$available" "$total" "$measured_volumes"
}

phase() {
  name=$1
  shift
  if [ "$name" = tools-image ]; then check_host_space; else check_resources; fi
  current_phase=$name
  printf 'Running %s\n' "$name"
  status=0
  if "$@" > "$results/$name.log" 2>&1; then :; else status=$?; fi
  printf '%s\t%s\t-\t-\t-\t-\t-\n' "$name" "$status" >> "$results/results.tsv"
  if [ "$status" -ne 0 ]; then
    failure_recorded=1
    printf 'Failed %s, exit=%s; see test-results/replica-catalog-storage.\n' "$name" "$status" >&2
    exit "$status"
  fi
}

run_tools() {
  service=$1
  shift
  compose run --rm --no-deps --pull never --interactive=false -T "$service" flock "$lock" "$@"
}

test_case() {
  name=$1
  expected=$2
  shift 2
  check_resources
  current_phase=$name
  printf 'Running %s\n' "$name"
  status=0
  if "$@" > "$results/$name.log" 2>&1; then :; else status=$?; fi
  counts=$(awk '/^test result: / {count++; values=$4 "\t" $6 "\t" $8 "\t" $10 "\t" $12} END {if (count != 1) exit 1; print values}' "$results/$name.log") || counts=$(printf -- '-\t-\t-\t-\t-')
  actual=$(printf '%s' "$counts" | tr '\t' ' ')
  if [ "$status" = 0 ] && [ "$actual" != "$expected" ]; then
    printf 'Failed %s: unexpected test counts (%s).\n' "$name" "$actual" >&2
    status=1
  fi
  printf '%s\t%s\t%s\n' "$name" "$status" "$counts" >> "$results/results.tsv"
  if [ "$status" -ne 0 ]; then
    failure_recorded=1
    printf 'Failed %s, exit=%s; see test-results/replica-catalog-storage.\n' "$name" "$status" >&2
    exit "$status"
  fi
  printf '%s: passed failed ignored measured filtered = %s\n' "$name" "$actual"
}

if [ -z "${HITO_CATALOG_STORAGE_TOOLS_IMAGE:-}" ]; then phase tools-image compose build rust; fi
if [ "$offline" = 0 ]; then phase dependencies run_tools storage-bootstrap cargo fetch --locked; fi
phase format run_tools rust rustfmt --edition 2024 --check "$entry"
phase strict run_tools rust cargo clippy --locked --offline -j 2 -p hito-core --test replica_catalog_storage -- -D warnings
test_case normal '2 0 1 0 0' run_tools rust cargo test --locked --offline -j 2 -p hito-core --test replica_catalog_storage -- --test-threads=1
test_case disk '1 0 0 0 2' compose run --rm --no-deps --pull never --interactive=false -T -e HITO_CATALOG_DISK_ROOT=/owned-disk-case rust flock "$lock" cargo test --locked --offline -j 2 -p hito-core --test replica_catalog_storage actual_disk_exhaustion -- --ignored --test-threads=1
echo "Qualification passed; results are in test-results/replica-catalog-storage."
