#!/bin/sh
set -eu
directory=${0%/*}
target=
for argument do
    target=$argument
done
printf '%s\n' 'private-extractor-stderr-sentinel' >&2
case "$target" in
    'https://music.youtube.com/search?q='*'#songs')
        printf '1\n' >> "$directory/calls"
        cat "$directory/search.json"
        ;;
    'https://www.youtube.com/watch?v=HitoMeta001')
        printf '2\n' >> "$directory/calls"
        cat "$directory/structured.json"
        ;;
    'https://www.youtube.com/watch?v=HitoMeta002')
        printf '3\n' >> "$directory/calls"
        cat "$directory/channel.json"
        ;;
    'https://www.youtube.com/watch?v=HitoCheck'[0-9][0-9])
        printf '4\n' >> "$directory/calls"
        id=${target##*=}
        cat "$directory/$id.json"
        ;;
    *) exit 1 ;;
esac
