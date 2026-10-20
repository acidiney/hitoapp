#!/bin/sh
set -eu
umask 077
cd "$(dirname "$0")/../.."
case "${1:-chromium}" in chromium|webkit) HITO_ARTIST_GROUPS_BROWSER=${1:-chromium} ;; *) exit 2 ;; esac
: "${HITO_PACKAGED_IMAGE:?Set the immutable packaged runtime image ID}"
: "${HITO_PACKAGED_SOURCE:?Set the exact packaged source commit}"
: "${HITO_PACKAGED_INPUTS:?Set the exact product input fingerprint}"
: "${HITO_PACKAGED_WORKTREE:?Set the exact frozen product checkout}"
: "${HITO_ARTIST_GROUPS_BROWSER_IMAGE:?Set the immutable Playwright 1.56.1 image ID}"
: "${HITO_ARTIST_GROUPS_NODE_IMAGE:?Set the immutable existing Node 22 image ID}"
HITO_ARTIST_GROUPS_WORKTREE=$(pwd)
hito_artist_run=$(mktemp -d "${TMPDIR:-/tmp}/hito-artist-identity-groups.XXXXXX")
chmod 755 "$hito_artist_run"
mkdir "$hito_artist_run/music" "$hito_artist_run/evidence"
chmod 777 "$hito_artist_run/music" "$hito_artist_run/evidence"
HITO_ARTIST_GROUPS_MUSIC="$hito_artist_run/music"
HITO_ARTIST_GROUPS_EVIDENCE="$hito_artist_run/evidence"
hito_artist_id=$(python3 -c 'import secrets; print(secrets.token_hex(16))')
hito_artist_project="hito-artist-groups-${HITO_ARTIST_GROUPS_BROWSER}-${hito_artist_id}"
hito_artist_tool_name="${hito_artist_project}-prepare"
export HITO_PACKAGED_IMAGE HITO_PACKAGED_SOURCE HITO_PACKAGED_INPUTS
export HITO_ARTIST_GROUPS_WORKTREE HITO_ARTIST_GROUPS_BROWSER HITO_ARTIST_GROUPS_BROWSER_IMAGE HITO_ARTIST_GROUPS_NODE_IMAGE
export HITO_ARTIST_GROUPS_MUSIC HITO_ARTIST_GROUPS_EVIDENCE
bounded() { python3 scripts/artist-identity-groups/tools.py run "$@"; }
compose_bounded() {
  hito_artist_timeout=$1
  shift
  bounded "$hito_artist_timeout" docker compose -p "$hito_artist_project" -f scripts/artist-identity-groups/compose.e2e.yaml --profile e2e --profile tools "$@"
}
compose() { compose_bounded 240 "$@"; }
cleanup_tool() {
  python3 scripts/artist-identity-groups/tools.py cleanup "$hito_artist_tool_name" "$hito_artist_project" "$hito_artist_id"
}
finish() {
  hito_artist_status=$?
  trap - EXIT HUP INT TERM
  set +e
  cleanup_tool > "$HITO_ARTIST_GROUPS_EVIDENCE/tool-cleanup.log" 2>&1
  hito_artist_tool_cleanup=$?
  compose_bounded 45 stop -t 15 artist-groups-browser artist-groups-server >/dev/null 2>&1
  hito_artist_stop=$?
  compose_bounded 30 rm -f artist-groups-browser artist-groups-server >/dev/null 2>&1
  hito_artist_remove=$?
  hito_artist_remaining=$(bounded 15 docker ps -aq --filter "label=com.docker.compose.project=$hito_artist_project")
  hito_artist_list=$?
  if test "$hito_artist_tool_cleanup" -ne 0 || test "$hito_artist_stop" -ne 0 || test "$hito_artist_remove" -ne 0 || test "$hito_artist_list" -ne 0 || test -n "$hito_artist_remaining"; then
    if test "$hito_artist_status" -eq 0; then hito_artist_status=1; fi
  fi
  printf 'Owned artist group final status=%s evidence=%s\n' "$hito_artist_status" "$HITO_ARTIST_GROUPS_EVIDENCE"
  exit "$hito_artist_status"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
python3 scripts/artwork/source-pin.py --root "$HITO_PACKAGED_WORKTREE" --expect-head "$HITO_PACKAGED_SOURCE" --expect-digest "$HITO_PACKAGED_INPUTS" --output "$HITO_ARTIST_GROUPS_EVIDENCE/product-source.json"
bounded 30 docker image inspect "$HITO_PACKAGED_IMAGE" > "$HITO_ARTIST_GROUPS_EVIDENCE/product-image.json"
python3 scripts/artwork/check-image.py "$HITO_ARTIST_GROUPS_EVIDENCE/product-image.json" "$HITO_PACKAGED_IMAGE" "$HITO_PACKAGED_SOURCE" "$HITO_PACKAGED_INPUTS"
python3 - "$HITO_ARTIST_GROUPS_BROWSER_IMAGE" "$HITO_ARTIST_GROUPS_NODE_IMAGE" <<'PYPINS'
import re, sys
if any(not re.fullmatch(r"sha256:[0-9a-f]{64}", value) for value in sys.argv[1:]):
    raise SystemExit("Owned artist browser tools require immutable image identities")
PYPINS
bounded 30 docker image inspect "$HITO_ARTIST_GROUPS_BROWSER_IMAGE" > "$HITO_ARTIST_GROUPS_EVIDENCE/browser-image.json"
bounded 30 docker image inspect "$HITO_ARTIST_GROUPS_NODE_IMAGE" > "$HITO_ARTIST_GROUPS_EVIDENCE/node-image.json"
printf '%s\n' "$hito_artist_project" > "$HITO_ARTIST_GROUPS_EVIDENCE/project.txt"
compose config --format json > "$HITO_ARTIST_GROUPS_EVIDENCE/compose.json"
compose_bounded 90 run -T --rm --name "$hito_artist_tool_name" --label "org.hito.fixture-run=$hito_artist_id" --label org.hito.owned-fixture=artist-identity-groups-browser --no-deps --pull never artist-groups-node > "$HITO_ARTIST_GROUPS_EVIDENCE/prepare.log" 2>&1
compose up -d --no-build --pull never --wait artist-groups-server
bounded 30 docker inspect "$(compose ps -q artist-groups-server)" > "$HITO_ARTIST_GROUPS_EVIDENCE/server-inspect.json"
compose create --no-build --no-recreate --pull never artist-groups-browser
bounded 30 docker inspect "$(compose ps -aq artist-groups-browser)" > "$HITO_ARTIST_GROUPS_EVIDENCE/browser-inspect.json"
hito_artist_status=0
compose up --no-deps --no-build --no-recreate --pull never --abort-on-container-exit --exit-code-from artist-groups-browser artist-groups-browser > "$HITO_ARTIST_GROUPS_EVIDENCE/browser.log" 2>&1 || hito_artist_status=$?
if test "$hito_artist_status" -eq 0; then
  python3 - "$HITO_ARTIST_GROUPS_EVIDENCE" <<'PYRESULT' || hito_artist_status=1
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
def bounded_json(name):
    path = root / name
    if path.stat().st_size > 524288:
        raise SystemExit("Owned browser evidence exceeded its bound")
    return json.loads(path.read_text())
report = bounded_json("result.json")
stats = report["stats"]
if stats["expected"] != 1 or any(stats[key] for key in ("unexpected", "flaky", "skipped")):
    raise SystemExit("The owned artist group browser did not complete exactly one passing case")
counts = bounded_json("artist-groups.json")["counts"]
errors = ("foreign", "audio", "download", "sourceWrites", "unexpectedWrites", "invalidDownloadStatus", "pageErrors", "malformedLogs", "privateConsole", "bounds", "drainTimeout")
if any(counts[key] for key in errors) or any(counts[key] != expected for key, expected in (("acceptedChoices", 4), ("failedChoices", 1), ("acceptedResets", 4), ("chooseRequests", 5), ("resetRequests", 4))):
    raise SystemExit("Owned artist group evidence did not satisfy its required workflow outcomes")
if counts["reviewStarts"] < 20 or counts["reviewStarts"] != counts["reviewFinishes"] or counts["downloadStatusReads"] < 1 or counts["downloadStatusReads"] != counts["downloadStatusAbsentReplies"]:
    raise SystemExit("Owned artist group evidence is incomplete")
PYRESULT
fi
printf 'Owned artist group browser status=%s evidence=%s\n' "$hito_artist_status" "$HITO_ARTIST_GROUPS_EVIDENCE"
exit "$hito_artist_status"
