#!/bin/sh
set -eu
directory=${0%/*}
printf '%s\n' invocation >> "$directory/calls"
if [ -e "$directory/fail" ]; then
    cat "$directory/stderr" >&2
    exit 1
fi
cat <<'JSON'
{"entries":[{"id":"HitoTone001","title":"Owned tone one","artist":"Hito test performer","duration":12.5,"url":"https://audio.invalid/owned?signature=private-signed-url-sentinel"}]}
JSON
