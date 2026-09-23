#!/usr/bin/env python3
"""Draws PoE2 Oracle's mark and writes every raster form the product ships.

The mark: an oracle's eye in a gold-rimmed dark medallion -- an amber, lit orb (the game's currency
orbs) with a slit pupil, set in a gold eyelid outline. Sizes below 32 px drop the eyelid outline and
enlarge the orb, following Windows' guidance to simplify 16-24 px icons instead of shrinking detail
into mush.

Pure standard library and deterministic, so the committed files can always be regenerated:

    lane exec -- python3 packaging/icon/generate_icon.py

Outputs, in crates/poe2-oracle/assets/icon/:
  poe2-oracle.ico      16/20/24/32/40/48/64 px as 32-bit BMP entries plus 256 px as PNG -- the exe's
                       icon (crates/poe2-oracle/build.rs) and the installer's (packaging/installer.nsi)
  tray-32.rgba         the 32 px mark as straight RGBA8, row-major from the top-left: the exact input
                       of `tray_icon::Icon::from_rgba` (crates/poe2-oracle/src/brand.rs)
  poe2-oracle-256.png  the 256 px mark, for documentation and release pages

Shapes are signed distance fields, so every edge is anti-aliased analytically at each size rather
than by downscaling one master image. Colours come from the app's shared palette
(crates/poe2-oracle/src/ui/theme.rs: GOLD 0xd0913b, TIER_TOP 0xecc94b, RARITY_UNIQUE 0xaf6025,
BG_PANEL 0x0e0e10).
"""

import math
import struct
import zlib
from pathlib import Path

OUT_DIR = Path(__file__).resolve().parents[2] / "crates" / "poe2-oracle" / "assets" / "icon"
ICO_SIZES = (16, 20, 24, 32, 40, 48, 64, 256)
TRAY_SIZE = 32


def rgb(hex_color):
    return tuple(((hex_color >> shift) & 0xFF) / 255 for shift in (16, 8, 0))


def ramp(stops, t):
    """Piecewise-linear colour ramp over `(position, colour)` stops sorted by position."""
    if t <= stops[0][0]:
        return stops[0][1]
    for (t0, c0), (t1, c1) in zip(stops, stops[1:]):
        if t <= t1:
            f = (t - t0) / (t1 - t0)
            return tuple(a + (b - a) * f for a, b in zip(c0, c1))
    return stops[-1][1]


RIM = [
    (0.00, rgb(0xF8DE95)),
    (0.40, rgb(0xECC94B)),
    (0.62, rgb(0xD0913B)),
    (1.00, rgb(0x6E4418)),
]
# At 16-24 px the rim is one or two pixels wide; its dark lower half would vanish against a dark
# taskbar, so small sizes stop the ramp at a mid gold.
RIM_SMALL = [(0.00, rgb(0xF8DE95)), (0.45, rgb(0xECC94B)), (1.00, rgb(0xB57A30))]
# A thin dark edge outside the rim keeps the pale top of the rim readable on light backgrounds.
OUTLINE = rgb(0x2A1A0B)
WELL = [(0.00, rgb(0x2E2317)), (0.65, rgb(0x16110C)), (1.00, rgb(0x0E0E10))]
SCLERA = [(0.00, rgb(0x100B07)), (1.00, rgb(0x2C1E11))]
IRIS = [
    (0.00, rgb(0xFFF3CC)),
    (0.22, rgb(0xF6D374)),
    (0.52, rgb(0xE3A442)),
    (0.80, rgb(0xAF6025)),
    (1.00, rgb(0x6A3212)),
]
GLOW = rgb(0xD0913B)
EYELID = [(0.00, rgb(0xF6D98C)), (1.00, rgb(0xC0852F))]
PUPIL = rgb(0x120B06)
SHINE = (1.0, 1.0, 1.0)


def circle(x, y, cx, cy, r):
    return math.hypot(x - cx, y - cy) - r


def ellipse(x, y, cx, cy, a, b):
    """Approximate signed distance to an axis-aligned ellipse; exact enough at the edge for AA."""
    px, py = (x - cx) / a, (y - cy) / b
    k0 = math.hypot(px, py)
    k1 = math.hypot(px / a, py / b)
    if k1 == 0.0:
        return -min(a, b)
    return k0 * (k0 - 1.0) / k1


def render(size):
    """The mark at `size` x `size` as straight RGBA8 rows, top row first."""
    half = size / 2
    px = 1 / half  # one pixel, in the unit coordinates below (-1..1 edge to edge)
    detailed = size >= 32

    r_edge = 1 - max(0.03, 0.6 * px)
    r_out = r_edge - max(0.018, 0.55 * px)
    r_in = r_out - max(0.12, 1.3 * px)
    rim = RIM if detailed else RIM_SMALL
    if detailed:
        # Eyelids: a lens between two circle arcs meeting at (+-lens_a, 0), reaching +-lens_b.
        lens_a, lens_b = 0.70, 0.38
        lens_k = (lens_a**2 - lens_b**2) / (2 * lens_b)
        lens_r = lens_k + lens_b
        lid_width = max(0.055, 1.1 * px)
        iris_r = 0.42
        pupil_a, pupil_b = max(0.07, 0.9 * px), 0.30
        shine = (-0.15, -0.17, max(0.075, 0.9 * px), 0.9)
        glow_strength = 0.35
    else:
        iris_r = 0.52
        pupil_a, pupil_b = max(0.10, 0.8 * px), 0.34
        shine = (-0.19, -0.21, max(0.09, 0.7 * px), 0.85)
        glow_strength = 0.2

    def coverage(distance):
        return min(1.0, max(0.0, 0.5 - distance / px))

    rows = []
    for j in range(size):
        y = (j + 0.5) / half - 1
        row = bytearray()
        for i in range(size):
            x = (i + 0.5) / half - 1
            # Premultiplied accumulator, composited back to front with "over".
            acc = [0.0, 0.0, 0.0, 0.0]

            def over(color, alpha):
                if alpha <= 0.0:
                    return
                keep = 1 - alpha
                for c in range(3):
                    acc[c] = color[c] * alpha + acc[c] * keep
                acc[3] = alpha + acc[3] * keep

            r = math.hypot(x, y)
            over(OUTLINE, coverage(r - r_edge))
            # Gold rim: a vertical metal ramp, lit from the top.
            over(ramp(rim, (y / r_out + 1) / 2), coverage(r - r_out))
            # The dark well inside the rim, with the orb's glow spilling into it.
            well = coverage(r - r_in)
            over(ramp(WELL, r / r_in), well)
            over(GLOW, well * glow_strength * max(0.0, 1 - r / r_in) ** 2)

            iris_d = circle(x, y, 0, 0, iris_r)
            if detailed:
                lens_d = max(math.hypot(x, y - lens_k), math.hypot(x, y + lens_k)) - lens_r
                inside_lids = coverage(lens_d)
                over(ramp(SCLERA, (y / lens_b + 1) / 2), inside_lids)
            else:
                inside_lids = 1.0
            # The orb: lit from the upper left, so the ramp is centred off-axis.
            lit = math.hypot(x + 0.14 * iris_r, y + 0.16 * iris_r) / (iris_r * 1.12)
            over(ramp(IRIS, lit), coverage(iris_d) * inside_lids)
            over(PUPIL, coverage(ellipse(x, y, 0, 0, pupil_a, pupil_b)) * inside_lids)
            sx, sy, sr, salpha = shine
            over(SHINE, coverage(circle(x, y, sx, sy, sr)) * salpha * inside_lids)
            if detailed:
                over(ramp(EYELID, (y / lens_b + 1) / 2), coverage(abs(lens_d) - lid_width / 2))

            alpha = acc[3]
            if alpha > 0.0:
                pixel = [acc[c] / alpha for c in range(3)] + [alpha]
            else:
                pixel = [0.0, 0.0, 0.0, 0.0]
            row.extend(round(min(1.0, max(0.0, v)) * 255) for v in pixel)
        rows.append(bytes(row))
    return rows


def png(rows, size):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    raw = b"".join(b"\x00" + row for row in rows)  # filter type 0 on every scanline
    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)  # 8-bit RGBA, no interlace
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def ico_bitmap(rows, size):
    """A classic ICO entry: BITMAPINFOHEADER, bottom-up BGRA, then the 1-bpp AND mask."""
    header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    color = bytearray()
    for row in reversed(rows):
        for i in range(0, len(row), 4):
            r, g, b, a = row[i : i + 4]
            color += bytes((b, g, r, a))
    mask_stride = (size + 31) // 32 * 4
    mask = bytearray()
    for row in reversed(rows):
        bits = bytearray(mask_stride)
        for i in range(size):
            if row[i * 4 + 3] == 0:
                bits[i // 8] |= 0x80 >> (i % 8)
        mask += bits
    return header + bytes(color) + bytes(mask)


def ico(images):
    """ICONDIR + entries; 256 px is stored as PNG (Vista+), smaller sizes as bitmaps."""
    directory = struct.pack("<HHH", 0, 1, len(images))
    offset = len(directory) + 16 * len(images)
    entries, blobs = b"", b""
    for size, data in images:
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset)
        blobs += data
        offset += len(data)
    return directory + entries + blobs


def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    renders = {size: render(size) for size in ICO_SIZES}
    images = [
        (size, png(rows, size) if size >= 256 else ico_bitmap(rows, size))
        for size, rows in renders.items()
    ]
    (OUT_DIR / "poe2-oracle.ico").write_bytes(ico(images))
    (OUT_DIR / "poe2-oracle-256.png").write_bytes(png(renders[256], 256))
    (OUT_DIR / "tray-32.rgba").write_bytes(b"".join(renders[TRAY_SIZE]))
    for name in ("poe2-oracle.ico", "poe2-oracle-256.png", "tray-32.rgba"):
        print(f"wrote {OUT_DIR / name} ({(OUT_DIR / name).stat().st_size} bytes)")


if __name__ == "__main__":
    main()
