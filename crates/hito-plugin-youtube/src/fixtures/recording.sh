#!/bin/sh
set -eu
directory=${0%/*}
printf '%s\n' invocation >> "$directory/calls"
printf '%s\0' "$@" >> "$directory/arguments"
cat "/proc/$$/environ" > "$directory/environment"
printf '%s\n' 'private-stderr-sentinel' >&2
cat "$directory/recording.json"
