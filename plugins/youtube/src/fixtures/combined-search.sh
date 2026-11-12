#!/bin/sh
set -eu
directory=${0%/*}
printf '%s\n' invocation >> "$directory/calls"
printf '%s\0' "$@" > "$directory/last-argv"
targets=false
failed=0
for argument in "$@"; do
    if [ "$targets" = false ]; then
        if [ "$argument" = "--" ]; then
            targets=true
        fi
        continue
    fi
    case "$argument" in
        'https://music.youtube.com/search?q='*) origin=songs ;;
        ytsearch*) origin=videos ;;
        *) exit 2 ;;
    esac
    mode=ok
    if [ -e "$directory/$origin.mode" ]; then
        mode=$(cat "$directory/$origin.mode")
    fi
    case "$mode" in
        ok)
            cat "$directory/$origin.json"
            printf '\n'
            ;;
        fail)
            printf '%s\n' 'ERROR: private-diagnostic-sentinel' >&2
            failed=1
            ;;
        malformed)
            printf '%s\n' '{"entries":[{"id":"private-truncated-sentinel'
            ;;
        hang)
            printf '%s\n' "$$" > "$directory/hanging.pid"
            remaining=1000
            while [ ! -e "$directory/release" ] && [ "$remaining" -gt 0 ]; do
                sleep 0.02
                remaining=$((remaining - 1))
            done
            ;;
    esac
done
exit "$failed"
