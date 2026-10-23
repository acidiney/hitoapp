#!/bin/sh
set -eu
directory=${0%/*}
printf '%s\n' invocation >> "$directory/calls"
if [ -e "$directory/fail" ]; then
    printf '%s\n' 'private-stderr-sentinel https://audio.invalid/owned?signature=private-signed-url-sentinel' >&2
    exit 7
fi
cat <<'JSON'
{"entries":[{"id":"HitoTone001","title":"private-title-sentinel","artist":"private-artist-sentinel","duration":12.5,"url":"https://audio.invalid/owned?signature=private-signed-url-sentinel","private_raw_json":"private-raw-json-sentinel"}]}
JSON
