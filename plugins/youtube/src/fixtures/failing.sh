#!/bin/sh
set -eu
directory=${0%/*}
cat "$directory/stderr" >&2
exit "$(cat "$directory/status")"
