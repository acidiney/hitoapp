#!/bin/sh
set -eu
directory=${0%/*}
if [ "${1-}" = "--owned-descendant" ]; then
    operation=$2
    printf '%s\n' "$$" > "$operation/child.pid"
    cat "/proc/$$/stat" > "$operation/child.identity"
    : > "$operation/child.ready"
    remaining=500
    while [ ! -e "$directory/release" ] && [ "$remaining" -gt 0 ]; do
        sleep 0.02
        remaining=$((remaining - 1))
    done
    exit 0
fi
printf '%s\n' invocation >> "$directory/calls"
if mkdir "$directory/first" 2>/dev/null; then
    operation=$directory/first
elif mkdir "$directory/second" 2>/dev/null; then
    operation=$directory/second
else
    exit 7
fi
printf '%s\n' "$$" > "$operation/leader.pid"
cat "/proc/$$/stat" > "$operation/leader.identity"
"$0" --owned-descendant "$operation" </dev/null >/dev/null 2>&1 &
remaining=300
while [ ! -e "$operation/child.ready" ]; do
    if [ "$remaining" -eq 0 ]; then
        exit 1
    fi
    sleep 0.01
    remaining=$((remaining - 1))
done
: > "$operation/leader.ready"
remaining=500
while [ ! -e "$directory/release" ] && [ "$remaining" -gt 0 ]; do
    sleep 0.02
    remaining=$((remaining - 1))
done
cat <<'JSON'
{"entries":[{"id":"HitoTone001","title":"Owned held tone","artist":"Hito test performer","duration":12.5}]}
JSON
exit 0
