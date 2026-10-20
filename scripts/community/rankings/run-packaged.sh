#!/bin/sh
set -eu
umask 077
cd "$(dirname "$0")/../../.."
case "${1:-chromium}" in chromium|webkit) HITO_RANKING_BROWSER=${1:-chromium} ;; *) exit 2 ;; esac
: "${HITO_PACKAGED_IMAGE:?Set the exact new Hito runtime image ID}"
: "${HITO_PACKAGED_SOURCE:?Set the exact Hito image source commit}"
: "${HITO_PACKAGED_INPUTS:?Set the exact Hito product input fingerprint}"
: "${HITO_RANKING_SERVICE_IMAGE:?Set the exact current community image ID}"
: "${HITO_RANKING_SERVICE_SOURCE:?Set the community image source commit}"
: "${HITO_RANKING_SERVICE_INPUTS:?Set the community service input fingerprint}"
HITO_RANKING_WORKTREE=$(pwd)
hito_ranking_run=$(mktemp -d "${TMPDIR:-/tmp}/hito-community-ranking-browser.XXXXXX")
chmod 777 "$hito_ranking_run"
mkdir "$hito_ranking_run/evidence"
chmod 777 "$hito_ranking_run/evidence"
HITO_RANKING_MUSIC="$hito_ranking_run/music"
HITO_RANKING_SECRETS="$hito_ranking_run/secrets"
HITO_RANKING_EVIDENCE="$hito_ranking_run/evidence"
export HITO_PACKAGED_IMAGE HITO_PACKAGED_SOURCE HITO_PACKAGED_INPUTS HITO_RANKING_BROWSER
export HITO_RANKING_SERVICE_IMAGE HITO_RANKING_SERVICE_SOURCE HITO_RANKING_SERVICE_INPUTS
export HITO_RANKING_WORKTREE HITO_RANKING_MUSIC HITO_RANKING_SECRETS HITO_RANKING_EVIDENCE
hito_ranking_id=$(python3 -c 'import secrets; print(secrets.token_hex(16))')
hito_ranking_project="hito-community-ranking-${HITO_RANKING_BROWSER}-${hito_ranking_id}"
printf '%s\n' "$hito_ranking_project" > "$HITO_RANKING_EVIDENCE/project.txt"
compose() { docker compose -p "$hito_ranking_project" -f scripts/community/rankings/compose.e2e.yaml --profile e2e "$@"; }
toolnode() { docker compose -p "$hito_ranking_project" -f scripts/artwork/compose.tools.yaml --profile tools run -T --rm --no-deps --pull never --label org.hito.owned-fixture=community-ranking-browser "$@"; }
capture() {
  compose logs --no-color --no-log-prefix community-ranking-server > "$HITO_RANKING_EVIDENCE/server.log" 2>&1 || true
  compose logs --no-color --no-log-prefix community-ranking-service > "$HITO_RANKING_EVIDENCE/community.log" 2>&1 || true
}
finish() {
  hito_ranking_incoming=$?
  trap - EXIT HUP INT TERM
  set +e
  sh scripts/community/rankings/cleanup.sh "$hito_ranking_project" "$HITO_RANKING_EVIDENCE" "$hito_ranking_incoming"
  hito_ranking_final=$?
  printf 'Owned community ranking final status=%s evidence=%s\n' "$hito_ranking_final" "$HITO_RANKING_EVIDENCE"
  exit "$hito_ranking_final"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
python3 scripts/artwork/source-pin.py --expect-head "$HITO_PACKAGED_SOURCE" --expect-digest "$HITO_PACKAGED_INPUTS" --output "$HITO_RANKING_EVIDENCE/app-source.json"
python3 scripts/community/rankings/service-source-pin.py --expect-digest "$HITO_RANKING_SERVICE_INPUTS" --output "$HITO_RANKING_EVIDENCE/service-source.json"
docker image inspect "$HITO_PACKAGED_IMAGE" > "$HITO_RANKING_EVIDENCE/app-image.json"
docker image inspect "$HITO_RANKING_SERVICE_IMAGE" > "$HITO_RANKING_EVIDENCE/service-image.json"
python3 scripts/artwork/check-image.py "$HITO_RANKING_EVIDENCE/app-image.json" "$HITO_PACKAGED_IMAGE" "$HITO_PACKAGED_SOURCE" "$HITO_PACKAGED_INPUTS"
python3 scripts/artwork/check-image.py "$HITO_RANKING_EVIDENCE/service-image.json" "$HITO_RANKING_SERVICE_IMAGE" "$HITO_RANKING_SERVICE_SOURCE" "$HITO_RANKING_SERVICE_INPUTS"
toolnode --user 10001:10001 -v "$hito_ranking_run:/owned" node node /workspace/scripts/community/rankings/prepare.mjs /owned
hito_ranking_compose_status=0
compose config --format json > "$HITO_RANKING_EVIDENCE/compose.json" || hito_ranking_compose_status=$?
if ! python3 - "$HITO_RANKING_EVIDENCE" "$hito_ranking_compose_status" <<'PYRECEIPT'
import hashlib, json, pathlib, sys
try:
    root = pathlib.Path(sys.argv[1])
    received = (root / "compose.json").read_bytes()
    receipt = {"file": "compose", "exit": int(sys.argv[2]), "bytes": len(received), "sha256": hashlib.sha256(received).hexdigest()}
    (root / "compose-writer.json").write_text(json.dumps(receipt) + "\n")
except (OSError, ValueError):
    print("Owned compose writer receipt failed", file=sys.stderr)
    sys.exit(1)
PYRECEIPT
then
  if test "$hito_ranking_compose_status" -eq 0; then hito_ranking_compose_status=1; fi
fi
if test "$hito_ranking_compose_status" -ne 0; then exit "$hito_ranking_compose_status"; fi
compose up -d --no-build --pull never --wait community-ranking-service
docker inspect "$(compose ps -q community-ranking-server)" > "$HITO_RANKING_EVIDENCE/server-inspect.json"
docker inspect "$(compose ps -q community-ranking-service)" > "$HITO_RANKING_EVIDENCE/service-inspect.json"
compose create --no-build --no-recreate --pull never community-ranking-browser
docker inspect "$(compose ps -aq community-ranking-browser)" > "$HITO_RANKING_EVIDENCE/browser-inspect.json"
toolnode -v "$HITO_RANKING_EVIDENCE:/evidence" node node /workspace/scripts/community/rankings/verify-evidence.mjs --preflight
hito_ranking_status=0
compose up --no-deps --no-build --no-recreate --pull never --abort-on-container-exit --exit-code-from community-ranking-browser community-ranking-browser > "$HITO_RANKING_EVIDENCE/browser.log" 2>&1 || hito_ranking_status=$?
if ! compose stop -t 15 community-ranking-service community-ranking-server >/dev/null; then
  if test "$hito_ranking_status" -eq 0; then hito_ranking_status=1; fi
fi
capture
if test "$hito_ranking_status" -eq 0; then
  toolnode -v "$HITO_RANKING_EVIDENCE:/evidence" node node /workspace/scripts/community/rankings/verify-evidence.mjs || hito_ranking_status=1
fi
if ! cat "$HITO_RANKING_EVIDENCE/browser.log"; then
  if test "$hito_ranking_status" -eq 0; then hito_ranking_status=1; fi
fi
printf 'Owned community ranking browser status=%s evidence=%s\n' "$hito_ranking_status" "$HITO_RANKING_EVIDENCE"
exit "$hito_ranking_status"
