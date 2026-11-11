#!/usr/bin/env python3
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys


def run(directory):
    directory.mkdir()
    audio = directory / "content-gap.mp3"
    command = [
        "ffmpeg", "-nostdin", "-v", "error", "-n", "-filter_threads", "1",
        "-f", "lavfi", "-i", "sine=frequency=330:sample_rate=44100:duration=60",
        "-af", "volume=4,volume=0:enable='between(t,2,2.2)',pan=stereo|c0=c0|c1=c0",
        "-c:a", "libmp3lame", "-b:a", "128k", "-threads", "1", str(audio),
    ]
    subprocess.run(command, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL, timeout=60, check=True)
    peaks = []
    for start in [1.8, 2.05, 2.3]:
        decoded = subprocess.run([
            "ffmpeg", "-nostdin", "-v", "error", "-ss", str(start), "-i", str(audio),
            "-t", "0.1", "-f", "f32le", "-c:a", "pcm_f32le", "-threads", "1", "pipe:1",
        ], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            timeout=10, check=True).stdout
        if not decoded or len(decoded) > 100_000 or len(decoded) % 4:
            raise RuntimeError("control_decode_size_invalid")
        peaks.append(max(abs(sample[0]) for sample in struct.iter_unpack("<f", decoded)))
    if not (peaks[0] > 0.2 and peaks[1] < 1e-4 and peaks[2] > 0.2):
        raise RuntimeError("control_signal_invalid")
    if audio.stat().st_size > 2 * 1024 * 1024:
        raise RuntimeError("control_audio_size_invalid")
    result = {
        "duration_seconds": 60, "content_silence_seconds": [2, 2.2],
        "decoded_window_starts_seconds": [1.8, 2.05, 2.3], "decoded_peak_amplitudes": peaks,
        "bytes": audio.stat().st_size, "sha256": hashlib.sha256(audio.read_bytes()).hexdigest(),
    }
    with (directory / "fixture-pins.json").open("x") as output:
        json.dump(result, output, indent=2)
        output.write("\n")
    print(json.dumps({"event": "owned_recovery_control_generated", **result}))


if __name__ == "__main__":
    arguments = argparse.ArgumentParser()
    arguments.add_argument("--directory", required=True, type=Path)
    options = arguments.parse_args()
    try:
        run(options.directory)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(json.dumps({"event": "recovery_control_infrastructure_failed", "kind": type(error).__name__}))
        sys.exit(1)
