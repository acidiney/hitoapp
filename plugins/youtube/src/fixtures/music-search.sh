#!/bin/sh
set -eu
directory=${0%/*}
printf '%s\0' "$@" > "$directory/last-argv"
printf '%s\n' invocation >> "$directory/calls"
for argument in "$@"; do
  case "$argument" in
    'https://www.youtube.com/watch?v=zzzzzzzzzzz')
      cat "$directory/recording.json"
      exit 0
      ;;
  esac
done
cat "$directory/search.json"
