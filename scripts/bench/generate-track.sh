#!/bin/sh
set -eu
index=$1
root=${BENCH_LIBRARY_ROOT:-/music}
duration=${BENCH_TRACK_SECONDS:-20}
case $((index % 3)) in
  0) format=FLAC; extension=flac; codec="-c:a flac" ;;
  1) format=MP3; extension=mp3; codec="-c:a libmp3lame -b:a 128k" ;;
  2) format=M4A; extension=m4a; codec="-c:a aac -b:a 128k -movflags +faststart" ;;
esac
artist=$(printf 'Bench Artist %03d' $((index / 20)))
album=$(printf 'Bench Album %03d-%d' $((index / 20)) $(((index % 20) / 10)))
title=$(printf 'Bench %s %05d' "$format" "$index")
track=$((index % 10 + 1))
directory="$root/Music/$artist/$album"
mkdir -p "$directory"
output="$directory/$(printf '%02d' "$track") $title.$extension"
frequency=$(awk -v i="$index" 'BEGIN { printf "%.2f", 200 + i * 0.1 }')
ffmpeg -v error -y -f lavfi -i "sine=frequency=$frequency:sample_rate=44100:duration=$duration" -af "volume=4,pan=stereo|c0=c0|c1=c0" \
  -metadata "title=$title" -metadata "artist=$artist" -metadata "album=$album" -metadata "album_artist=$artist" \
  -metadata "track=$track" -metadata "genre=Bench" $codec "$output"
