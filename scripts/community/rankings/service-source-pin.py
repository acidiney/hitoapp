#!/usr/bin/env python3
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

INPUTS = ["docker/community.Dockerfile", ".dockerignore", "LICENSE", "Cargo.toml", "Cargo.lock",
          "rust-toolchain.toml", ".cargo", "apps/community", "vendor", "docs/third-party/sqlx-sqlite.md"]

parser = argparse.ArgumentParser()
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--expect-digest")
arguments = parser.parse_args()
root = Path(__file__).resolve().parents[3]
subprocess.run(["git", "-C", str(root), "diff", "--quiet", "HEAD", "--", *INPUTS], check=True)
tree = subprocess.check_output(["git", "-C", str(root), "ls-tree", "-rz", "--full-tree", "HEAD", "--", *INPUTS])
files = []
for entry in tree.split(b"\0"):
    if not entry:
        continue
    metadata, name = entry.split(b"\t", 1)
    mode, kind, _object = metadata.split(b" ")
    if kind != b"blob" or mode not in (b"100644", b"100755"):
        raise SystemExit("Community image inputs must be ordinary committed files")
    path = name.decode("utf8")
    contents = (root / path).read_bytes()
    files.append([path, mode.decode(), len(contents), hashlib.sha256(contents).hexdigest()])
files.sort(key=lambda file: file[0])
digest = hashlib.sha256(json.dumps(files, ensure_ascii=False, separators=(",", ":")).encode()).hexdigest()
if arguments.expect_digest and digest != arguments.expect_digest:
    raise SystemExit("Community service inputs differ from the immutable artifact")
arguments.output.write_text(json.dumps({"schema_version": 1, "community_inputs_sha256": digest, "files": files}, indent=2) + "\n")
print(json.dumps({"community_inputs_sha256": digest, "files": len(files)}))
