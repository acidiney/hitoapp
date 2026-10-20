#!/usr/bin/env python3
"""Bound owned Docker commands and remove only an exact preparation container."""

import json
import re
import subprocess
import sys


def run(arguments, seconds, maximum=5 * 1024 * 1024):
    result = subprocess.run(
        arguments, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=seconds
    )
    if len(result.stdout) > maximum:
        raise ValueError("Owned command output exceeded its bound")
    return result


def cleanup(name, project, run_id):
    if (
        not re.fullmatch(r"[0-9a-f]{32}", run_id)
        or not re.fullmatch(r"hito-artist-groups-(chromium|webkit)-" + run_id, project)
        or name != f"{project}-prepare"
    ):
        raise ValueError("Invalid exact preparation ownership")
    labels = {
        "com.docker.compose.project": project,
        "org.hito.fixture-run": run_id,
        "org.hito.owned-fixture": "artist-identity-groups-browser",
    }
    found = run(["docker", "inspect", name], 30, 1024 * 1024)
    if found.returncode == 0:
        containers = json.loads(found.stdout)
        if (
            len(containers) != 1
            or containers[0].get("Name") != f"/{name}"
            or any(
                containers[0].get("Config", {}).get("Labels", {}).get(key) != value
                for key, value in labels.items()
            )
            or not re.fullmatch(r"[0-9a-f]{64}", containers[0].get("Id", ""))
        ):
            raise ValueError("The exact owned preparation ownership changed")
        if run(["docker", "rm", "-f", containers[0]["Id"]], 30).returncode:
            raise ValueError("The exact owned preparation removal failed")
    filters = [
        argument
        for key, value in labels.items()
        for argument in ["--filter", f"label={key}={value}"]
    ]
    remaining = run(["docker", "ps", "-aq", *filters], 30)
    if remaining.returncode or remaining.stdout.strip():
        raise ValueError("An exact owned preparation survived cleanup")


def main():
    if len(sys.argv) == 5 and sys.argv[1] == "cleanup":
        cleanup(*sys.argv[2:])
        return 0
    if len(sys.argv) < 4 or sys.argv[1] != "run":
        raise ValueError("Invalid owned tool command")
    seconds = int(sys.argv[2])
    if not 1 <= seconds <= 300:
        raise ValueError("Invalid owned tool deadline")
    result = run(sys.argv[3:], seconds)
    sys.stdout.buffer.write(result.stdout)
    return result.returncode


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, OSError, KeyError, json.JSONDecodeError, subprocess.TimeoutExpired):
        print("Owned artist tool command or exact cleanup failed", file=sys.stderr)
        raise SystemExit(1) from None
