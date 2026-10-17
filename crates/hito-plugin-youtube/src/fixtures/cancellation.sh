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
selector=
for argument in "$@"; do
    selector=$argument
done
case "$selector" in
    ytsearch1:private-first-query-sentinel) slot=first ;;
    ytsearch1:private-second-query-sentinel) slot=second ;;
    ytsearch1:private-fourth-query-sentinel)
        printf '%s\n' '{"entries":[{"id":"HitoTone004","title":"Owned resumed tone","artist":"Hito test performer","duration":12.5}]}'
        exit 0
        ;;
    *) exit 7 ;;
esac
operation=$directory/$slot
mkdir "$operation"
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
printf '%s\n' '{"entries":[{"id":"HitoTone001","title":"Owned held tone","artist":"Hito test performer","duration":12.5}]}'
exit 0
