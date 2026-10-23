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
cat "/proc/$$/stat" > "$directory/leader.identity"
"$0" --owned-descendant </dev/null >/dev/null 2>&1 &
remaining=300
while [ ! -e "$directory/child.ready" ]; do
    if [ "$remaining" -eq 0 ]; then
        exit 1
    fi
    sleep 0.01
    remaining=$((remaining - 1))
done
printf '%s\n' 'private-stderr-sentinel https://audio.invalid/owned?signature=private-signed-url-sentinel' >&2
printf '%s\n' '{"entries":[{"id":"HitoTone001","title":"private-title-sentinel","artist":"private-artist-sentinel","private_raw_json":"private-raw-json-sentinel"}]}'
: > "$directory/leader.ready"
remaining=500
while [ ! -e "$directory/release" ] && [ "$remaining" -gt 0 ]; do
    sleep 0.02
    remaining=$((remaining - 1))
done
exit 0
