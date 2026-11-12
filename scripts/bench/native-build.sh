#!/bin/sh
set -u
lock=$1
shift
exec 9>"$lock"
flock -x 9 || exit 1
printf '{"locked":true}\n'
IFS= read -r control || exit 1
[ "$control" = start ] || exit 1
exec 8<&0
setsid timeout --signal=TERM --kill-after=5s 1800s "$@" &
compiler=$!
cancel() {
  kill -TERM "-$compiler" 2>/dev/null || true
}
trap cancel HUP INT TERM
(
  exec 9>&-
  exec >/dev/null 2>&1
  IFS= read -r control <&8 || control=abort
  cancel
  sleep 5
  kill -KILL "-$compiler" 2>/dev/null || true
) &
watcher=$!
status=0
wait "$compiler" || status=$?
if kill -0 "$compiler" 2>/dev/null; then
  cancel
  sleep 5
  kill -KILL "-$compiler" 2>/dev/null || true
  wait "$compiler" 2>/dev/null || true
fi
kill "$watcher" 2>/dev/null || true
wait "$watcher" 2>/dev/null || true
exit "$status"
