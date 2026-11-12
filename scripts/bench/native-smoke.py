#!/usr/bin/env python3
import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time


def write_json(path, value):
    with path.open("x") as output:
        json.dump(value, output, indent=2)
        output.write("\n")


def digest(path):
    return {"bytes": path.stat().st_size, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def fixtures(directory):
    music = directory / "Music"
    music.mkdir()
    specifications = [
        ("FLAC", "flac", ["-c:a", "flac"]),
        ("MP3", "mp3", ["-c:a", "libmp3lame", "-b:a", "128k"]),
        ("M4A", "m4a", ["-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"]),
    ]
    generated = {}
    for index, (format_name, extension, codec) in enumerate(specifications):
        title = f"Bench {format_name} {index:05}"
        path = music / f"owned-{index}.{extension}"
        encode(path, 6, 200 + index * 0.1, codec, [
            "-metadata", f"title={title}", "-metadata", "artist=Bench Artist 000",
            "-metadata", "album=Bench Album 000-0", "-metadata", "album_artist=Bench Artist 000",
            "-metadata", f"track={index + 1}", "-metadata", "genre=Bench",
        ])
        generated[f"Music/{path.name}"] = {**digest(path), "duration_seconds": 6, "format": format_name}
    remote = directory / "remote.mp3"
    encode(remote, 60, 330, ["-c:a", "libmp3lame", "-b:a", "128k"], [])
    generated[remote.name] = {**digest(remote), "duration_seconds": 60, "format": "MP3"}
    if len(generated) != 4 or sum(item["bytes"] for item in generated.values()) > 4 * 1024 * 1024:
        raise RuntimeError("fixture_size_bound")
    write_json(directory / "fixture-pins.json", generated)
    return music, remote


def encode(path, seconds, frequency, codec, metadata):
    command = [
        "ffmpeg", "-nostdin", "-v", "error", "-n", "-filter_threads", "1",
        "-f", "lavfi", "-i", f"sine=frequency={frequency}:sample_rate=44100:duration={seconds}",
        "-af", "volume=4,pan=stereo|c0=c0|c1=c0", *metadata, *codec, "-threads", "1", str(path),
    ]
    subprocess.run(command, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL, timeout=60, check=True)


def run(binary, directory):
    directory.mkdir(mode=0o755)
    fixture_directory = directory / "fixtures"
    fixture_directory.mkdir()
    music, remote = fixtures(fixture_directory)
    output = directory / "results"
    output.mkdir(mode=0o777)
    output.chmod(0o777)
    environment = os.environ.copy()
    settings = {
        "BENCH_TRACKS": "3", "BENCH_SAMPLES": "1", "BENCH_WARMUP": "0", "BENCH_RTT_MS": "40",
        "BENCH_LIBRARY": str(music), "BENCH_AUDIO": str(remote), "BENCH_OUT": str(output),
        "BENCH_PROVIDER": "/workspace/apps/web/e2e/latency/fake-provider.mjs",
    }
    environment.update(settings)
    command = ["sh", "/workspace/scripts/desktop/with-audio.sh", str(binary)]
    write_json(directory / "runtime-preflight.json", {
        "argv": command, "binary": digest(binary), "environment": settings,
        "fixture_pins": digest(fixture_directory / "fixture-pins.json"),
        "scope": "one debug smoke attempt per nine series; monitor read detection; no p95 or physical output proof",
    })
    started = time.monotonic()
    expired = False
    with (directory / "main.log").open("xb") as log:
        process = subprocess.Popen(command, env=environment, stdin=subprocess.DEVNULL,
                                   stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = process.wait(timeout=240)
        except subprocess.TimeoutExpired:
            expired = True
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
        code = process.returncode
    rows = []
    samples = output / "native-samples.csv"
    if samples.is_file():
        with samples.open() as recorded:
            rows = list(csv.DictReader(recorded))
    expected = {
        ("local_start", "FLAC", "0"), ("local_start", "MP3", "0"), ("local_start", "M4A", "0"),
        ("remote_start", "direct", "0"), ("remote_start", "direct", "40"),
        ("recovery", "cached_alternate", "0"), ("recovery", "cached_alternate", "40"),
        ("recovery", "refreshed_alternate", "0"), ("recovery", "refreshed_alternate", "40"),
    }
    seen = [(row["scenario"], row["variant"], row["rtt_ms"]) for row in rows]
    complete = len(seen) == 9 and set(seen) == expected and all(row["sample"] == "0" for row in rows)
    run_status = None
    if (output / "native-run.json").is_file():
        run_status = json.loads((output / "native-run.json").read_text())
    summary = {
        "exit": code, "outer_deadline_exceeded": expired, "seconds": time.monotonic() - started,
        "measured_rows": len(rows), "failed_rows": sum(row["ok"] != "1" for row in rows),
        "nine_series_complete": complete, "run_status": run_status,
        "files": {str(path.relative_to(directory)): digest(path) for path in directory.rglob("*") if path.is_file()},
    }
    write_json(directory / "smoke-result.json", summary)
    print(json.dumps({key: summary[key] for key in ["exit", "outer_deadline_exceeded", "seconds", "measured_rows", "failed_rows", "nine_series_complete", "run_status"]}))
    return 0 if code == 0 and not expired and complete and summary["failed_rows"] == 0 else 1


def main():
    arguments = argparse.ArgumentParser()
    arguments.add_argument("--binary", type=Path, required=True)
    arguments.add_argument("--directory", type=Path, required=True)
    options = arguments.parse_args()
    def interrupted(_number, _frame):
        raise InterruptedError("smoke_interrupted")
    signal.signal(signal.SIGINT, interrupted)
    signal.signal(signal.SIGTERM, interrupted)
    try:
        return run(options.binary, options.directory)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError, RuntimeError) as error:
        print(json.dumps({"event": "native_smoke_infrastructure_failed", "kind": type(error).__name__}))
        return 1


if __name__ == "__main__":
    sys.exit(main())
