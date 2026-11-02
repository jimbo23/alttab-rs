#!/usr/bin/env python3
"""Render AppIcon.svg -> PNGs and pack AppIcon.icns (PNG-encoded entries).

Requires `resvg` (cargo install resvg) on PATH. Pure-Python ICNS writer, no Apple tools.
Usage: python3 assets/icon/build_icon.py
"""
import os, struct, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
SVG = os.path.join(HERE, "AppIcon.svg")
OUT = os.path.join(HERE, "build")
# ICNS OSType -> pixel size (PNG payload is accepted by macOS 10.7+ for all of these)
ENTRIES = [
    ("icp4", 16), ("icp5", 32), ("icp6", 64),
    ("ic07", 128), ("ic08", 256), ("ic09", 512), ("ic10", 1024),
    ("ic11", 32), ("ic12", 64), ("ic13", 256), ("ic14", 512),
]

def render(size: int) -> str:
    path = os.path.join(OUT, f"icon_{size}.png")
    if not os.path.exists(path):
        subprocess.run(["resvg", "-w", str(size), "-h", str(size), SVG, path], check=True)
    return path

def main() -> int:
    os.makedirs(OUT, exist_ok=True)
    chunks = b""
    for ostype, size in ENTRIES:
        data = open(render(size), "rb").read()
        chunks += ostype.encode("ascii") + struct.pack(">I", len(data) + 8) + data
    icns = b"icns" + struct.pack(">I", len(chunks) + 8) + chunks
    with open(os.path.join(HERE, "AppIcon.icns"), "wb") as f:
        f.write(icns)
    # Keep the 1024 preview at a stable path
    import shutil
    shutil.copyfile(render(1024), os.path.join(HERE, "AppIcon-1024.png"))
    print(f"wrote AppIcon.icns ({len(icns)} bytes) and AppIcon-1024.png")
    return 0

if __name__ == "__main__":
    sys.exit(main())
