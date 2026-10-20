#!/usr/bin/env python3
"""Build only the verified frozen context after checking resolved Compose inputs."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

sys.dont_write_bytecode = True

from source import ROOT, verify


def resolve(manifest):
    verify(manifest)
    context = Path(manifest["context"])
    if not context.is_absolute() or context.resolve() != ROOT:
        raise ValueError("Run the build helper from this manifest's frozen context")
    revision, digest = manifest["source_id"], manifest["input_sha256"]
    if not re.fullmatch(r"[0-9a-f]{40}", revision) or not re.fullmatch(r"[0-9a-f]{64}", digest):
        raise ValueError("Build identities must be immutable revision and input digests")
    project = "hito-all-debrid-package-" + digest[:12]
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith(("HITO_ALL_DEBRID_", "COMPOSE_"))}
    environment.update({
        "COMPOSE_PROJECT_NAME": project,
        "HITO_ALL_DEBRID_BUILD_CONTEXT": str(context),
        "HITO_ALL_DEBRID_SOURCE_ID": revision,
        "HITO_ALL_DEBRID_INPUT_SHA256": digest,
        "HITO_ALL_DEBRID_IMAGE_TAG": digest,
        "HITO_ALL_DEBRID_CREDENTIAL_VOLUME": "not-used-during-build",
        "HITO_ALL_DEBRID_LOG_LEVEL": "info",
        "HITO_ALL_DEBRID_PORT": "8097",
    })
    command = ["docker", "compose", "--project-directory", str(context), "--env-file", os.devnull,
               "-p", project, "-f", str(context / "compose.all-debrid.yaml"), "--profile", "plugins"]
    resolved = subprocess.run(command + ["config", "--format", "json"], env=environment,
                              capture_output=True, timeout=20, check=True)
    configuration = json.loads(resolved.stdout)
    service = configuration["services"]["plugin-all-debrid"]
    expected_build = {
        "context": str(context), "dockerfile": "Dockerfile.plugin-all-debrid",
        "args": {"HITO_ALL_DEBRID_SOURCE_ID": revision, "HITO_ALL_DEBRID_INPUT_SHA256": digest,
                 "HITO_PLUGIN_CACHE_ID": "hito-plugin-all-debrid-" + project},
    }
    if service["build"] != expected_build or service["image"] != "hito-plugin-all-debrid:" + digest:
        raise ValueError("Resolved Compose build inputs differ from the verified manifest")
    verify(manifest)
    return command, environment, configuration


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--snapshot", required=True, type=Path)
    parser.add_argument("--check-only", action="store_true", help="Resolve and verify config without compiling")
    arguments = parser.parse_args()
    manifest = json.loads(arguments.snapshot.read_text())
    command, environment, configuration = resolve(manifest)
    if arguments.check_only:
        print(json.dumps(configuration, indent=2))
        return 0
    print(json.dumps({"event": "all_debrid_package_build_verified", "source_id": manifest["source_id"],
                      "input_sha256": manifest["input_sha256"],
                      "image": configuration["services"]["plugin-all-debrid"]["image"]}), flush=True)
    try:
        completed = subprocess.run(command + ["build", "--pull=false", "plugin-all-debrid"],
                                   env=environment, check=False)
    finally:
        verify(manifest)
    return completed.returncode


if __name__ == "__main__":
    sys.exit(main())
