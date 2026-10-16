#!/usr/bin/env python3
"""Generate owned synthetic tones with truthful RIFF tags and no external media."""
import math
import pathlib
import struct
import sys


def chunk(name, data):
    return name + struct.pack("<I", len(data)) + data + (b"\0" if len(data) % 2 else b"")


def generate(folder):
    folder.mkdir(parents=True, exist_ok=True)
    for frequency in (440, 554, 659):
        samples = bytearray()
        rate = 16000
        duration = 20
        for index in range(rate * duration):
            fade = min(1.0, index / 800, (rate * duration - index) / 800)
            value = int(4000 * fade * math.sin(2 * math.pi * frequency * index / rate))
            samples.extend(struct.pack("<h", value))
        info = b"INFO"
        for key, value in (
            (b"INAM", f"Hito demo tone {frequency} Hz"),
            (b"IART", "Hito synthetic demo"),
            (b"IPRD", "Generated test signals"),
            (b"ICMT", "Original synthetic sine wave generated for Hito. No external recording."),
        ):
            info += chunk(key, value.encode() + b"\0")
        body = b"WAVE"
        body += chunk(b"fmt ", struct.pack("<HHIIHH", 1, 1, rate, rate * 2, 2, 16))
        body += chunk(b"LIST", info)
        body += chunk(b"data", samples)
        audio = b"RIFF" + struct.pack("<I", len(body)) + body
        (folder / f"signal-{frequency}.wav").write_bytes(audio)


if __name__ == "__main__":
    generate(pathlib.Path(sys.argv[1]))
