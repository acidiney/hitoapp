#!/usr/bin/env python3
"""Freeze the exact build context and packaging policy without running Docker."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess


ROOT = Path(__file__).resolve().parents[3]
FILES = {
    ".dockerignore", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "LICENSE",
    "docker/plugin.Dockerfile", "deploy/compose/all-debrid.yaml",
}
PREFIXES = (".cargo/", "apps/server/", "apps/desktop/", "apps/community/", "crates/", "plugins/", "vendor/", "scripts/debrid/all-debrid/")


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def identity(path):
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
        raise ValueError("Source inputs must be regular single-link files")
    return {"sha256": sha256(path.read_bytes()), "mode": stat.S_IMODE(metadata.st_mode)}


def verify(manifest):
    payload = {key: manifest[key] for key in ("source_id", "files")}
    if sha256(canonical(payload)) != manifest["input_sha256"]:
        raise ValueError("Source manifest digest changed")
    context = Path(manifest["context"])
    actual = {str(path.relative_to(context)) for path in context.rglob("*") if path.is_file()}
    if actual != set(manifest["files"]):
        raise ValueError("Frozen context file membership changed")
    for name, expected in manifest["files"].items():
        if identity(context / name) != expected:
            raise ValueError("Frozen source input changed")
    return manifest


def freeze(destination):
    destination = destination.absolute()
    destination.mkdir(mode=0o700)
    context = destination / "context"
    context.mkdir(mode=0o700)
    tracked = subprocess.check_output(
        ["git", "ls-files", "-z"], cwd=ROOT
    ).decode().split("\0")
    names = {name for name in tracked if name in FILES or name.startswith(PREFIXES)}
    names.update(FILES)
    namespace = ROOT / "scripts/debrid/all-debrid"
    names.update(str(path.relative_to(ROOT)) for path in namespace.rglob("*") if path.is_file())
    files = {}
    for name in sorted(names):
        source = ROOT / name
        files[name] = identity(source)
        output = context / name
        output.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, output)
        output.chmod(files[name]["mode"])
    source_id = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip()
    payload = {"source_id": source_id, "files": files}
    manifest = dict(payload, input_sha256=sha256(canonical(payload)), context=str(context))
    verify(manifest)
    (destination / "source.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"source_id": source_id, "input_sha256": manifest["input_sha256"],
                      "file_count": len(files), "snapshot": str(destination)}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("freeze").add_argument("destination", type=Path)
    commands.add_parser("verify").add_argument("manifest", type=Path)
    arguments = parser.parse_args()
    os.umask(0o077)
    if arguments.command == "freeze":
        freeze(arguments.destination)
    else:
        manifest = verify(json.loads(arguments.manifest.read_text()))
        print(json.dumps({"verified_files": len(manifest["files"]),
                          "input_sha256": manifest["input_sha256"]}))


if __name__ == "__main__":
    main()
