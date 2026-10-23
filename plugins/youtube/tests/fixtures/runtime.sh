#!/bin/sh
set -eu
directory=${0%/*}
cat "/proc/$$/environ" > "$directory/environment"
printf '%s\0' "$@" > "$directory/arguments"
printf '%s\n' '{"entries":[{"id":"HitoTone001","title":"Owned runtime tone","artist":"Hito test performer"}]}'
