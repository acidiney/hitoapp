#!/bin/sh
set -eu
directory=${0%/*}
printf '%s\n' invocation "$@" >> "$directory/argv"
cat <<'JSON'
{
  "_type": "playlist",
  "extractor_key": "YoutubeSearch",
  "entries": [
    {
      "id": "HitoTone001",
      "title": "Owned tone one",
      "artist": "Hito test performer",
      "duration": 12.5,
      "url": "https://audio.invalid/one?token=private-upstream-token",
      "webpage_url": "https://www.youtube.com/watch?v=HitoTone001"
    },
    {
      "id": "HitoTone002",
      "title": "Owned tone two",
      "artist": "Hito test performer",
      "duration": 13,
      "url": "https://audio.invalid/two?token=private-upstream-token"
    },
    {
      "id": "HitoTone003",
      "title": "Owned extra result",
      "artist": "Hito test performer",
      "duration": 14,
      "url": "https://audio.invalid/three?token=private-upstream-token"
    }
  ]
}
JSON
