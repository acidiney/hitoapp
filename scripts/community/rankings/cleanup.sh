#!/bin/sh
set -uf
umask 077
project=${1:-}
evidence=${2:-}
incoming=${3:-0}
case "$incoming" in ''|*[!0-9]*) exit 2 ;; esac
if test "$incoming" -gt 255; then exit 2; fi
case "$project" in
  hito-community-ranking-chromium-*) suffix=${project#hito-community-ranking-chromium-} ;;
  hito-community-ranking-webkit-*) suffix=${project#hito-community-ranking-webkit-} ;;
  *) if test "$incoming" -ne 0; then exit "$incoming"; else exit 2; fi ;;
esac
case "$suffix" in *[!0-9a-f]*) if test "$incoming" -ne 0; then exit "$incoming"; else exit 2; fi ;; esac
if test "${#suffix}" -ne 32 || ! test -d "$evidence"; then
  if test "$incoming" -ne 0; then exit "$incoming"; else exit 2; fi
fi
initial=0
rejected=0
stop_failed=0
remove_failed=0
capture_failed=0
inventory_failed=0
survivors=-1
owned() {
  case "$1" in *[!0-9a-f]*) return 1 ;; esac
  test "${#1}" -eq 64 || return 1
  metadata=$(docker inspect --format '{{.Id}} {{.Image}} {{index .Config.Labels "com.docker.compose.project"}} {{index .Config.Labels "com.docker.compose.service"}} {{index .Config.Labels "org.hito.owned-fixture"}} {{index .Config.Labels "com.docker.compose.oneoff"}}' "$1" 2>> "$evidence/cleanup-docker.log") || return 1
  expected_id=$1
  set -- $metadata
  test "$#" -eq 6 || return 1
  test "$1" = "$expected_id" && test "$3" = "$project" && test "$5" = community-ranking-browser || return 1
  case "$2" in sha256:????????????????????????????????????????????????????????????????) ;; *) return 1 ;; esac
  case "$4:$6" in
    community-ranking-server:False|community-ranking-service:False|community-ranking-browser:False|node:True) ;;
    *) return 1 ;;
  esac
  owned_service=$4
  return 0
}
inventory() {
  docker ps -a --no-trunc --filter "label=com.docker.compose.project=$project" --format '{{.ID}}'
}
if inventory > "$evidence/cleanup-initial.ids" 2>> "$evidence/cleanup-docker.log"; then
  while IFS= read -r id; do
    initial=$((initial + 1))
    if ! owned "$id"; then rejected=$((rejected + 1)); continue; fi
    before=$metadata
    if test "$owned_service" != node; then
      if ! docker stop -t 15 "$id" >> "$evidence/cleanup-docker.log" 2>&1; then
        stop_failed=$((stop_failed + 1))
      fi
      case "$owned_service" in
        community-ranking-server) log=server.log ;;
        community-ranking-service) log=community.log ;;
        *) log=cleanup-browser.log ;;
      esac
      if docker logs "$id" > "$evidence/$log.cleanup-capture" 2>&1; then
        if ! mv "$evidence/$log.cleanup-capture" "$evidence/$log"; then capture_failed=$((capture_failed + 1)); fi
      else
        capture_failed=$((capture_failed + 1))
      fi
    fi
    if ! owned "$id" || test "$metadata" != "$before"; then
      rejected=$((rejected + 1))
      continue
    fi
    if ! docker rm -f "$id" >> "$evidence/cleanup-docker.log" 2>&1; then
      remove_failed=$((remove_failed + 1))
    fi
  done < "$evidence/cleanup-initial.ids"
else
  inventory_failed=$((inventory_failed + 1))
fi
if inventory > "$evidence/cleanup-survivors.ids" 2>> "$evidence/cleanup-docker.log"; then
  survivors=$(wc -l < "$evidence/cleanup-survivors.ids" | tr -d ' ')
else
  inventory_failed=$((inventory_failed + 1))
fi
status=$incoming
if test "$status" -eq 0 && { test "$rejected" -ne 0 || test "$stop_failed" -ne 0 || test "$remove_failed" -ne 0 || test "$inventory_failed" -ne 0 || test "$survivors" -ne 0; }; then
  status=1
fi
if ! printf '{"event":"owned_cleanup","incoming":%s,"exit":%s,"initial":%s,"ownershipRejected":%s,"stopFailed":%s,"removeFailed":%s,"captureFailed":%s,"inventoryFailed":%s,"survivors":%s}\n' "$incoming" "$status" "$initial" "$rejected" "$stop_failed" "$remove_failed" "$capture_failed" "$inventory_failed" "$survivors" > "$evidence/cleanup-result.json"; then
  if test "$status" -eq 0; then status=1; fi
fi
exit "$status"
