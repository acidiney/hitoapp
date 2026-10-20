#!/bin/sh
set -eu
umask 077
cd "$(dirname "$0")/../../.."
entry=${HITO_RANKING_CLEANUP_ENTRY:-scripts/community/rankings/cleanup.sh}
evidence=$(mktemp -d "${TMPDIR:-/tmp}/hito-ranking-cleanup-control.XXXXXX")
real_docker=$(command -v docker)
HITO_CLEANUP_CONTROL_IMAGE=$(docker image inspect node:22-bookworm-slim --format '{{.Id}}')
export HITO_CLEANUP_CONTROL_IMAGE
case "$HITO_CLEANUP_CONTROL_IMAGE" in sha256:????????????????????????????????????????????????????????????????) ;; *) exit 2 ;; esac
printf '%s\n' "$evidence"
compose() { "$real_docker" compose -p "$project" -f scripts/community/rankings/compose.cleanup-control.yaml "$@"; }
inventory() { "$real_docker" ps -a --no-trunc --filter "label=com.docker.compose.project=$project" --format '{{.ID}}'; }
start() {
  project="hito-community-ranking-chromium-$(python3 -c 'import secrets; print(secrets.token_hex(16))')"
  case_dir="$evidence/$1"
  mkdir "$case_dir"
  printf '%s\n' "$project" > "$case_dir/project.txt"
  compose up -d --no-build --pull never community-ranking-server community-ranking-service community-ranking-browser > "$case_dir/start.log" 2>&1
  compose run --rm -d --no-deps --pull never node > "$case_dir/oneoff.txt" 2>> "$case_dir/start.log"
  inventory > "$case_dir/before.ids"
  test "$(wc -l < "$case_dir/before.ids" | tr -d ' ')" -eq 4
}
recover() {
  inventory > "$case_dir/recovery.ids"
  while IFS= read -r id; do
    meta=$("$real_docker" inspect --format '{{.Id}} {{index .Config.Labels "com.docker.compose.project"}} {{index .Config.Labels "org.hito.owned-fixture"}}' "$id")
    test "$meta" = "$id $project community-ranking-browser"
    "$real_docker" rm -f "$id" >> "$case_dir/recovery.log" 2>&1
  done < "$case_dir/recovery.ids"
  test -z "$(inventory)"
}
start success
status=0
sh "$entry" "$project" "$case_dir" 0 > "$case_dir/caller.log" 2>&1 || status=$?
inventory > "$case_dir/after.ids"
printf '{"incoming":0,"exit":%s,"survivors":%s}\n' "$status" "$(wc -l < "$case_dir/after.ids" | tr -d ' ')" > "$case_dir/result.json"
if test "$status" -ne 0 || test -s "$case_dir/after.ids"; then
  recover
  exit 1
fi
start removal-failure
victim=$("$real_docker" inspect --format '{{.Id}}' "$(cat "$case_dir/oneoff.txt")")
printf '%s\n' "$victim" > "$case_dir/victim.txt"
mkdir "$case_dir/bin"
cat > "$case_dir/bin/docker" <<'WRAPPER'
#!/bin/sh
if test "$#" -eq 3 && test "$1" = rm && test "$2" = -f && test "$3" = "$HITO_CONTROL_VICTIM"; then
  exec "$HITO_CONTROL_REAL_DOCKER" rm "$3"
fi
exec "$HITO_CONTROL_REAL_DOCKER" "$@"
WRAPPER
chmod 700 "$case_dir/bin/docker"
export HITO_CONTROL_VICTIM="$victim" HITO_CONTROL_REAL_DOCKER="$real_docker"
status=0
PATH="$case_dir/bin:$PATH" sh "$entry" "$project" "$case_dir" 0 > "$case_dir/caller.log" 2>&1 || status=$?
inventory > "$case_dir/after.ids"
"$real_docker" inspect --format '{{.Id}} {{.Image}} {{.State.Running}} {{index .Config.Labels "com.docker.compose.project"}} {{index .Config.Labels "com.docker.compose.service"}} {{index .Config.Labels "org.hito.owned-fixture"}}' "$victim" > "$case_dir/retained-victim.txt"
if test -f "$case_dir/cleanup-result.json"; then cp "$case_dir/cleanup-result.json" "$case_dir/cleanup-first-result.json"; fi
status7=0
PATH="$case_dir/bin:$PATH" sh "$entry" "$project" "$case_dir" 7 > "$case_dir/incoming7.log" 2>&1 || status7=$?
printf '{"incoming":0,"exit":%s,"preservedIncoming7":%s,"survivors":%s}\n' "$status" "$status7" "$(wc -l < "$case_dir/after.ids" | tr -d ' ')" > "$case_dir/result.json"
valid=0
if test "$status" -ne 0 && test "$status7" -eq 7 && test "$(cat "$case_dir/after.ids")" = "$victim"; then valid=1; fi
recover
if test "$valid" -ne 1; then exit 1; fi
printf '%s\n' 'Owned cleanup controls passed: zero survivors; real removal conflict nonzero; incoming7 preserved; exact-ID recovery complete'
