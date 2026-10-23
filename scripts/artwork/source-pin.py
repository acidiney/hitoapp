#!/usr/bin/env python3
"""Read the committed product inputs without including local browser fixture additions."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

PRODUCT_INPUTS = [
    "docker/server.Dockerfile", ".dockerignore", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml",
    ".cargo", "crates", "plugins", "vendor", "apps", "docs/openapi.json",
]


def git(*arguments):
    return subprocess.check_output(["git", *arguments])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--root", type=Path)
    parser.add_argument("--expect-head")
    parser.add_argument("--expect-digest")
    arguments = parser.parse_args()
    root = (arguments.root or Path(__file__).resolve().parents[2]).resolve()
    head = git("-C", str(root), "rev-parse", "HEAD").decode().strip()
    if arguments.expect_head and head != arguments.expect_head:
        raise SystemExit("The product checkout does not match the packaged source commit")
    subprocess.run(["git", "-C", str(root), "diff", "--quiet", "HEAD", "--", *PRODUCT_INPUTS], check=True)
    tree = git("-C", str(root), "ls-tree", "-rz", "--full-tree", "HEAD", "--", *PRODUCT_INPUTS)
    files = []
    for entry in tree.split(b"\0"):
        if not entry:
            continue
        metadata, raw_path = entry.split(b"\t", 1)
        mode, kind, _object = metadata.split(b" ")
        if kind != b"blob":
            raise SystemExit("A product input is not a committed file")
        path = raw_path.decode("utf-8")
        source = root / path
        contents = os.readlink(source).encode("utf-8") if mode == b"120000" else source.read_bytes()
        files.append([path, mode.decode(), len(contents), hashlib.sha256(contents).hexdigest()])
    files.sort(key=lambda item: item[0])
    serialized = json.dumps(files, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    digest = hashlib.sha256(serialized).hexdigest()
    if arguments.expect_digest and digest != arguments.expect_digest:
        raise SystemExit("The product inputs do not match the packaged image fingerprint")
    arguments.output.write_text(json.dumps({
        "schema_version": 1, "source_head": head, "product_inputs_sha256": digest,
        "files": files,
    }, indent=2) + "\n")
    print(json.dumps({"source": head, "product_inputs_sha256": digest, "files": len(files)}))


if __name__ == "__main__":
    main()
