#!/bin/sh
set -eu
directory=${0%/*}
if [ "${1-}" = "--owned-descendant" ]; then
    printf '%s\n' "$$" > "$directory/child.pid"
    cat "/proc/$$/stat" > "$directory/child.identity"
    : > "$directory/child.ready"
    remaining=500
    while [ ! -e "$directory/release" ] && [ "$remaining" -gt 0 ]; do
        sleep 0.02
        remaining=$((remaining - 1))
    done
    exit 0
fi
printf '%s\n' "$$" > "$directory/leader.pid"
"$0" --owned-descendant </dev/null >/dev/null 2>&1 &
remaining=300
while [ ! -e "$directory/child.ready" ]; do
    if [ "$remaining" -eq 0 ]; then
        exit 1
    fi
    sleep 0.01
    remaining=$((remaining - 1))
done
cat <<'JSON'
{"entries":[{"id":"HitoTone001","title":"Owned tone","artist":"Hito test performer","duration":12.5}]}
JSON
exit 0
