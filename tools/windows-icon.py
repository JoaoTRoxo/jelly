#!/usr/bin/env python3
"""Make data/windows/jelly.ico, the Windows executable's icon, from the app's
SVG icon: PNG images at each size Windows asks for, rendered by rsvg-convert.

    tools/windows-icon.py
"""
import struct
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parent.parent
svg = root / "data" / "io.github.joaotroxo.Jelly.svg"
sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
images = [
    subprocess.run(["rsvg-convert", "-w", str(s), "-h", str(s), str(svg)], check=True, capture_output=True).stdout
    for s in sizes
]
offset = 6 + 16 * len(images)
header = struct.pack("<HHH", 0, 1, len(images))
entries = b""
for size, png in zip(sizes, images):
    # Width and height 0 mean 256
    entries += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(png), offset)
    offset += len(png)
(root / "data" / "windows" / "jelly.ico").write_bytes(header + entries + b"".join(images))
