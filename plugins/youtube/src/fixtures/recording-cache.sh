#!/bin/sh
set -eu
directory=${0%/*}
last=
for argument do
    last=$argument
done
case "$last" in
    'https://www.youtube.com/watch?v='*) owner=${last#'https://www.youtube.com/watch?v='} ;;
    *) exit 2 ;;
esac
case "$owner" in
    ''|*[!A-Za-z0-9_-]*) exit 2 ;;
esac
[ "${#owner}" -eq 11 ] || exit 2
printf '%s\n' "$owner" >> "$directory/calls"
cat "$directory/recordings/$owner.json"
