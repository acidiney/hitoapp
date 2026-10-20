#!/usr/bin/env python3
"""Audit one frozen standalone image offline; never build or contact a provider."""

import argparse
import json
import os
from pathlib import Path
import re
import signal
import sys
import time

sys.dont_write_bytecode = True

from engine import Docker
from inputs import Inputs
from logs import metadata_lifecycle
from protocol import container_contract, image_contract, metadata_workflow, wait_healthy
from source import sha256, verify


def interrupted(_signal, _frame):
    raise KeyboardInterrupt()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image-id", required=True)
    parser.add_argument("--snapshot", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    arguments = parser.parse_args()
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", arguments.image_id):
        parser.error("Use the exact local immutable image ID")
    os.umask(0o077)
    arguments.output_dir.mkdir(mode=0o700)
    signal.signal(signal.SIGTERM, interrupted)
    started = time.monotonic()
    report = {"outcome": "failed", "image_id": arguments.image_id, "stage": "source_before"}
    docker = Docker(arguments.image_id)
    try:
        manifest = verify(json.loads(arguments.snapshot.read_text()))
        report.update(source_id=manifest["source_id"], input_sha256=manifest["input_sha256"],
                      source_files=len(manifest["files"]), owner=docker.owner)
        report["stage"] = "image_contract"
        image_contract(docker, manifest)
        inputs = Inputs()
        report["stage"] = "private_input_provision"
        docker.create_volume()
        docker.helper("provision", "provision", inputs.archive())
        before = inputs.validate_snapshot(docker.helper("inspect-inputs", "before"))
        report["stage"] = "restricted_service"
        container = docker.start()
        identifier = container["Id"]
        report["container_id"] = identifier
        container_contract(container, docker.volume)
        wait_healthy(docker, identifier)
        report["stage"] = "public_metadata"
        metadata_workflow(docker, identifier, inputs)
        report["stage"] = "idle_sigterm"
        stopped = docker.stop(identifier)
        report["exit_code"] = stopped["State"]["ExitCode"]
        report["stage"] = "private_input_preservation"
        after = inputs.validate_snapshot(docker.helper("inspect-inputs", "after"))
        if before != after:
            raise ValueError("Private input bytes or descriptor metadata changed")
        report["preserved_private_files"] = 4
        report["private_snapshot_sha256"] = sha256("\n".join(after).encode())
        report["stage"] = "trace_privacy"
        output = docker.call("logs", identifier, limit=65536, merge=True)
        (arguments.output_dir / "service.jsonl").write_bytes(output)
        report["trace"] = metadata_lifecycle(output)
        report["trace_sha256"] = sha256(output)
        report["outcome"] = "passed"
    except BaseException as error:
        report["failure"] = type(error).__name__
        if isinstance(error, (RuntimeError, ValueError)):
            report["reason"] = str(error)
    finally:
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        try:
            report["cleanup"] = docker.cleanup()
        except Exception as error:
            report["cleanup"] = {"failures": [type(error).__name__]}
        if report["cleanup"]["failures"]:
            report["outcome"] = "failed"
        try:
            verify(json.loads(arguments.snapshot.read_text()))
            report["source_after"] = "exact"
        except Exception:
            report["source_after"] = "failed"
            report["outcome"] = "failed"
        report["elapsed_ms"] = round((time.monotonic() - started) * 1000)
        report["commands"] = docker.commands
        (arguments.output_dir / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: report[key] for key in ("outcome", "stage", "image_id", "elapsed_ms")}))
    return 0 if report["outcome"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
