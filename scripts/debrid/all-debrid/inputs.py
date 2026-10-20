"""Synthetic private input archive and descriptor-preservation oracle."""

import io
import json
import secrets
import tarfile

from source import sha256


class Inputs:
    def __init__(self):
        self.prefix = "owned-private-package-" + secrets.token_hex(12)
        self.provider = self.prefix + "-provider"
        self.service = self.prefix + "-service"
        self.wrong = self.prefix + "-wrong"
        self.recording = {
            "id": self.prefix + "-recording", "title": self.prefix + "-title",
            "artist": self.prefix + "-artist", "duration_ms": 2000, "release": None,
            "disc_number": None, "track_number": None, "revision": self.prefix + "-revision",
        }
        catalog = {
            "media_origins": ["https://media.hito-package.invalid"],
            "recordings": [{
                "recording": self.recording, "source_id": self.prefix + "-source",
                "label": self.prefix + "-label",
                "binding": {
                    "magnet_id": 7123, "info_hash": "0123456789abcdef0123456789abcdef01234567",
                    "magnet_bytes": 384044, "file_bytes": 384044,
                    "file_path": [self.prefix + "-folder", self.prefix + ".wav"],
                },
            }],
        }
        self.files = {
            "all-debrid-provider-token": (self.provider.encode() + b"\r\n", 0o400),
            "all-debrid-service-token": (self.service.encode() + b"\n", 0o600),
            "all-debrid-receipt-key": (b"S" + secrets.token_bytes(31), 0o400),
            "all-debrid-catalog.json": (json.dumps(catalog).encode(), 0o600),
        }

    def archive(self):
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode="w") as archive:
            directory = tarfile.TarInfo(".")
            directory.type = tarfile.DIRTYPE
            directory.uid = directory.gid = 10001
            directory.mode = 0o700
            archive.addfile(directory)
            for name, (data, mode) in self.files.items():
                item = tarfile.TarInfo(name)
                item.uid = item.gid = 10001
                item.mode = mode
                item.size = len(data)
                archive.addfile(item, io.BytesIO(data))
        return output.getvalue()

    def validate_snapshot(self, data):
        lines = data.decode().splitlines()
        if len(lines) != 13 or set(lines[:4]) != set(self.files):
            raise ValueError("Private input membership changed")
        metadata = {}
        for line in lines[4:9]:
            name, kind, mode, uid, gid, links, size, device, inode, modified, changed = line.split("|")
            if name in metadata or (uid, gid) != ("10001", "10001"):
                raise ValueError("Private input owner changed")
            if name == ".":
                if (kind, mode) != ("directory", "700"):
                    raise ValueError("Private directory permissions changed")
            elif name in self.files:
                expected, expected_mode = self.files[name]
                if (kind, mode, links, size) != (
                    "regular file", format(expected_mode, "o"), "1", str(len(expected))
                ):
                    raise ValueError("Private file descriptor metadata changed")
            else:
                raise ValueError("Unexpected private input metadata")
            metadata[name] = line
        if set(metadata) != {"."}.union(self.files):
            raise ValueError("Private input descriptor metadata missing")
        checksums = dict((name, digest) for digest, name in (line.split("  ") for line in lines[9:]))
        if checksums != {name: sha256(data) for name, (data, _) in self.files.items()}:
            raise ValueError("Private input bytes changed")
        return sorted(lines[:4]) + lines[4:]
