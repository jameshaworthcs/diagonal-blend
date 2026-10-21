#!/usr/bin/env python3
"""Generate the synthetic test fixtures in tests/fixtures.

Standard library only, plus ffmpeg for the JPEG and cargo for the ICC
profile. The output is deterministic, so re-running it changes nothing.
"""

import math
import struct
import subprocess
import sys
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "tests" / "fixtures"


def png(path, w, h, pixel, icc=None):
    rows = bytearray()
    for y in range(h):
        rows.append(0)
        for x in range(w):
            rows.extend(pixel(x, y))

    def chunk(kind, data):
        c = struct.pack(">I", len(data)) + kind + data
        return c + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    out = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
    if icc is not None:
        out += chunk(b"iCCP", b"Display P3\x00\x00" + zlib.compress(icc, 9))
    out += chunk(b"IDAT", zlib.compress(bytes(rows), 9)) + chunk(b"IEND", b"")
    path.write_bytes(out)


def clamp(v):
    return max(0, min(255, int(round(v))))


def warm(x, y, w=1200, h=900):
    """Warm gradient with concentric rings and a fine grid: shows filtering."""
    u, v = x / w, y / h
    r = math.hypot(x - w * 0.42, y - h * 0.48)
    ring = 0.5 + 0.5 * math.cos(r / 9.0)
    grid = 0.82 if (x % 60 < 2 or y % 60 < 2) else 1.0
    return bytes(
        clamp(c * grid)
        for c in (235 - 60 * v, 120 + 90 * u * ring, 60 + 40 * ring)
    )


def cool(x, y, w=900, h=1200):
    """Cool gradient with a checkerboard and a bright diagonal bar."""
    u, v = x / w, y / h
    check = 1.0 if ((x // 75) + (y // 75)) % 2 == 0 else 0.86
    bar = 1.0 if abs((x - y * 0.75) - 120) < 18 else 0.0
    return bytes(
        clamp(c * check + 120 * bar)
        for c in (40 + 70 * u, 90 + 80 * v, 200 - 40 * u)
    )


def letter_f(x, y, w=600, h=400):
    """A stored-sideways 'F': it only reads correctly once rotated."""
    # The F is drawn as it should look after EXIF orientation 6 (rotate 90
    # degrees clockwise), so the stored image holds it turned the other way.
    dx, dy = y, w - 1 - x  # displayed coordinates, 400 x 600
    stem = 80 <= dx < 140 and 100 <= dy < 500
    top = 80 <= dx < 320 and 100 <= dy < 160
    mid = 80 <= dx < 260 and 270 <= dy < 330
    if stem or top or mid:
        return bytes((250, 250, 245))
    return bytes((30 + dx // 4, 60, 110 + dy // 8))


def exif_orientation(value):
    """APP1 segment holding only an orientation tag (big-endian TIFF)."""
    tiff = b"MM\x00\x2a" + struct.pack(">I", 8)
    tiff += struct.pack(">H", 1) + struct.pack(">HHIHH", 0x0112, 3, 1, value, 0) + struct.pack(">I", 0)
    body = b"Exif\x00\x00" + tiff
    return b"\xff\xe1" + struct.pack(">H", len(body) + 2) + body


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    png(OUT / "pattern-a.png", 1200, 900, warm)
    png(OUT / "pattern-b.png", 900, 1200, cool)

    stored = OUT / "rotated-6.png"
    png(stored, 600, 400, letter_f)
    jpg = OUT / "rotated-6.jpg"
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", "-i", str(stored), "-q:v", "2", str(jpg)],
        check=True,
    )
    stored.unlink()
    data = jpg.read_bytes()
    assert data[:2] == b"\xff\xd8"
    jpg.write_bytes(data[:2] + exif_orientation(6) + data[2:])

    icc = subprocess.run(
        ["cargo", "run", "-q", "--example", "icc", "--", "p3"],
        cwd=ROOT / "core",
        check=True,
        capture_output=True,
    ).stdout
    # Pure Display P3 red: outside sRGB, so it shows whether the gamut survives.
    png(OUT / "p3-red.png", 320, 240, lambda x, y: bytes((255, 0, 0)) if x < 160 else bytes((255, 255, 255)), icc)
    print("fixtures written to", OUT, file=sys.stderr)


if __name__ == "__main__":
    main()
