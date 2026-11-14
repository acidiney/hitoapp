#!/bin/sh
set -eu
docker compose --profile tools run --rm rust cargo test --locked -p hito-playback
docker compose --profile desktop run --rm desktop cargo clippy --locked -p hito-playback --features device --all-targets -- -D warnings
docker compose --profile desktop run --rm desktop cargo test --locked -p hito-playback --features device --lib pcm::tests
docker compose --profile tools run --rm desktop python3 scripts/desktop/test_audio_launcher.py -v
sh scripts/run-audio-test.sh hito-audio output_recovery --backend-fault-control "$@"
sh scripts/run-audio-test.sh hito-playback native_session "$@"
sh scripts/run-audio-test.sh hito-playback native_pcm_fallback "$@"
sh scripts/run-audio-test.sh hito-playback native_transport "$@"
sh scripts/run-audio-test.sh hito-playback native_transient_occurrence "$@"
sh scripts/run-audio-test.sh hito-playback native_pause_acquisition "$@"
sh scripts/run-audio-test.sh hito-playback native_terminal_occurrence "$@"
sh scripts/run-audio-test.sh hito-playback native_progress "$@"
sh scripts/run-audio-test.sh hito-playback native_saved "$@"
sh scripts/run-audio-test.sh hito-playback native_audiobook "$@"
sh scripts/run-audio-test.sh hito-playback player_logging "$@"
sh scripts/run-audio-test.sh hito-playback native_acquisition_clock "$@"
sh scripts/run-audio-test.sh hito-playback native_audio_network_occurrence "$@"
