#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
case "${1:-chromium}" in chromium|webkit) HITO_ARTWORK_BROWSER=${1:-chromium} ;; *) exit 2 ;; esac
: "${HITO_PACKAGED_IMAGE:?Set the immutable packaged runtime image ID}"
: "${HITO_PACKAGED_SOURCE:?Set the exact packaged source commit}"
: "${HITO_PACKAGED_INPUTS:?Set the packaged product input fingerprint}"
export HITO_ARTWORK_BROWSER HITO_PACKAGED_IMAGE HITO_PACKAGED_SOURCE HITO_PACKAGED_INPUTS
hito_artwork_run=$(mktemp -d "${TMPDIR:-/tmp}/hito-artwork-browser.XXXXXX")
chmod 755 "$hito_artwork_run"
mkdir "$hito_artwork_run/music" "$hito_artwork_run/evidence"
chmod 777 "$hito_artwork_run/evidence"
HITO_ARTWORK_MUSIC="$hito_artwork_run/music"
HITO_ARTWORK_EVIDENCE="$hito_artwork_run/evidence"
export HITO_ARTWORK_MUSIC HITO_ARTWORK_EVIDENCE
hito_artwork_project="hito-collection-artwork-${HITO_ARTWORK_BROWSER}-gate"
compose() {
  docker compose -p "$hito_artwork_project" -f scripts/artwork/compose.e2e.yaml --profile e2e "$@"
}
toolnode() {
  docker compose -p hito-collection-artwork-tools -f scripts/artwork/compose.tools.yaml --profile tools run -T --rm --no-deps --pull never "$@"
}
cleanup() {
  compose logs --no-color --no-log-prefix collection-artwork-server > "$HITO_ARTWORK_EVIDENCE/server.log" 2>&1 || true
  compose stop -t 15 collection-artwork-server >/dev/null 2>&1 || true
  compose rm -f collection-artwork-server >/dev/null 2>&1 || true
}
if test -n "$(docker ps -aq --filter "label=com.docker.compose.project=$hito_artwork_project")"; then
  echo "An owned artwork project is already active; refusing to replace it" >&2
  exit 1
fi
trap cleanup EXIT HUP INT TERM
python3 scripts/artwork/source-pin.py --expect-head "$HITO_PACKAGED_SOURCE" --expect-digest "$HITO_PACKAGED_INPUTS" --output "$HITO_ARTWORK_EVIDENCE/source-pin.json"
docker image inspect "$HITO_PACKAGED_IMAGE" > "$HITO_ARTWORK_EVIDENCE/image-inspect.json"
python3 scripts/artwork/check-image.py "$HITO_ARTWORK_EVIDENCE/image-inspect.json" "$HITO_PACKAGED_IMAGE" "$HITO_PACKAGED_SOURCE" "$HITO_PACKAGED_INPUTS"
toolnode -v "$HITO_ARTWORK_MUSIC:/owned-music" node node /workspace/scripts/artwork/prepare.mjs /owned-music
compose config --format json > "$HITO_ARTWORK_EVIDENCE/compose.json"
compose up -d --no-build --pull never --wait collection-artwork-server
hito_artwork_container=$(compose ps -q collection-artwork-server)
docker inspect "$hito_artwork_container" > "$HITO_ARTWORK_EVIDENCE/server-inspect.json"
toolnode -v "$HITO_ARTWORK_EVIDENCE:/evidence" node node /workspace/scripts/artwork/verify-evidence.mjs --preflight
HITO_ARTWORK_JS_SHA256=$(compose exec -T collection-artwork-server sh -c 'sha256sum /app/web/assets/app.js' | cut -d ' ' -f 1)
HITO_ARTWORK_CSS_SHA256=$(compose exec -T collection-artwork-server sh -c 'sha256sum /app/web/assets/app.css' | cut -d ' ' -f 1)
export HITO_ARTWORK_JS_SHA256 HITO_ARTWORK_CSS_SHA256
hito_artwork_status=0
compose run -T --rm --no-deps --pull never collection-artwork-browser > "$HITO_ARTWORK_EVIDENCE/browser.log" 2>&1 || hito_artwork_status=$?
compose logs --no-color --no-log-prefix collection-artwork-server > "$HITO_ARTWORK_EVIDENCE/server.log"
compose stop -t 15 collection-artwork-server >/dev/null
compose logs --no-color --no-log-prefix collection-artwork-server > "$HITO_ARTWORK_EVIDENCE/server.log"
if test "$hito_artwork_status" -eq 0; then
  toolnode -v "$HITO_ARTWORK_EVIDENCE:/evidence" node node /workspace/scripts/artwork/verify-evidence.mjs || hito_artwork_status=1
fi
cat "$HITO_ARTWORK_EVIDENCE/browser.log"
printf 'Owned artwork browser status=%s evidence=%s\n' "$hito_artwork_status" "$HITO_ARTWORK_EVIDENCE"
exit "$hito_artwork_status"
