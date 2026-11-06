#!/bin/sh
set -eu
root=${BENCH_LIBRARY_ROOT:-/music}
count=${BENCH_TRACKS:-10000}
jobs=${BENCH_GENERATOR_JOBS:-6}
here=$(dirname "$0")
mkdir -p "$root/Music" "$root/remote"
if [ -f "$root/ready" ] && [ "$(cat "$root/ready")" = "$count" ]; then
  echo "Library of $count tracks already present."
  exit 0
fi
rm -rf "$root/Music"
mkdir -p "$root/Music"
seq 0 $((count - 1)) | xargs -P "$jobs" -n 1 sh "$here/generate-track.sh"
ffmpeg -v error -y -f lavfi -i "sine=frequency=330:sample_rate=44100:duration=60" -af "volume=4,pan=stereo|c0=c0|c1=c0" -c:a libmp3lame -b:a 128k "$root/remote/remote.mp3"
printf '{"version":1,"directories":[{"id":"bench-music","name":"Music","root":"/music/Music"}]}' > "$root/storage.json"
printf '%s' "$count" > "$root/ready"
find "$root/Music" -type f | wc -l
du -sh "$root"
