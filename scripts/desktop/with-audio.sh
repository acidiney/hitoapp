#!/bin/sh
set -eu
test_root=$(mktemp -d)
sound_pid=""
trap 'if test -n "$sound_pid"; then kill "$sound_pid" 2>/dev/null || true; wait "$sound_pid" 2>/dev/null || true; fi; rm -rf "$test_root"' EXIT
mkdir -m 700 "$test_root/runtime"
chown -R 10001:10001 "$test_root"
socket="$test_root/pulse.sock"
runuser -u hito-desktop -- env XDG_RUNTIME_DIR="$test_root/runtime" pulseaudio -n --daemonize=no --exit-idle-time=-1 --use-pid-file=no --log-target=stderr --load="module-null-sink sink_name=hito-test rate=48000 channels=2" --load="module-native-protocol-unix socket=$socket auth-anonymous=1" > "$test_root/pulse.log" 2>&1 &
sound_pid=$!
for attempt in $(seq 1 100); do
  test ! -S "$socket" || break
  sleep 0.05
done
test -S "$socket"
printf "pcm.!default { type pulse }\nctl.!default { type pulse }\n" > "$test_root/asound.conf"
chown 10001:10001 "$test_root/asound.conf"
runuser -u hito-desktop -- env PULSE_SERVER="unix:$socket" ALSA_CONFIG_PATH="$test_root/asound.conf" "$@"
