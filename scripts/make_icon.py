#!/usr/bin/env python3
"""Build a multi-size ICO from assets/icon.png for the on99KB exe and window."""

from __future__ import annotations

import io
import struct
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "assets" / "icon.png"
DST = ROOT / "assets" / "icon.ico"
PREVIEW_DIR = ROOT / "assets" / "icon-previews"

SIZES = (16, 20, 24, 32, 48, 256)


def content_bbox(im: Image.Image, alpha_min: int = 16) -> tuple[int, int, int, int]:
    a = im.split()[-1]
    px = a.load()
    w, h = im.size
    xs, ys = [], []
    for y in range(h):
        for x in range(w):
            if px[x, y] >= alpha_min:
                xs.append(x)
                ys.append(y)
    if not xs:
        return (0, 0, w, h)
    return (min(xs), min(ys), max(xs) + 1, max(ys) + 1)


def crop_for_size(im: Image.Image, size: int) -> Image.Image:
    x0, y0, x1, y1 = content_bbox(im)
    # Drop empty left/right padding. For 16–24 px also tighten a little more
    # so the phone body + screen stay readable instead of a muddy full-frame blob.
    width = x1 - x0
    height = y1 - y0
    if size <= 24:
        inset_x = int(width * 0.04)
        inset_y = int(height * 0.02)
        x0 += inset_x
        x1 -= inset_x
        y0 += inset_y
        y1 -= inset_y
    else:
        pad = max(2, int(min(width, height) * 0.02))
        x0 = max(0, x0 - pad)
        y0 = max(0, y0 - pad)
        x1 = min(im.width, x1 + pad)
        y1 = min(im.height, y1 + pad)
    return im.crop((x0, y0, x1, y1))


def add_light_edge(im: Image.Image, lift: int = 28) -> Image.Image:
    """Slightly lift a dark body and add a 1px light edge for dark taskbars."""
    im = im.convert("RGBA")
    px = im.load()
    w, h = im.size
    # Lift dark, mostly-opaque body pixels.
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a < 40:
                continue
            luma = 0.299 * r + 0.587 * g + 0.114 * b
            if luma < 90:
                f = lift * (a / 255) * ((90 - luma) / 90)
                px[x, y] = (
                    min(255, int(r + f)),
                    min(255, int(g + f)),
                    min(255, int(b + f)),
                    a,
                )

    # Light outline on the outside of the opaque silhouette.
    alpha = im.split()[-1]
    dilated = alpha.filter(ImageFilter.MaxFilter(3))
    ring = Image.new("L", im.size, 0)
    rp = ring.load()
    ap = alpha.load()
    dp = dilated.load()
    for y in range(h):
        for x in range(w):
            if dp[x, y] >= 80 and ap[x, y] < 80:
                rp[x, y] = 220
    edge = Image.new("RGBA", im.size, (0, 0, 0, 0))
    ep = edge.load()
    for y in range(h):
        for x in range(w):
            t = rp[x, y]
            if t:
                ep[x, y] = (230, 230, 235, t)
    return Image.alpha_composite(edge, im)


def render_size(src: Image.Image, size: int) -> Image.Image:
    cropped = crop_for_size(src, size)
    # Keep aspect; letterbox on a transparent square.
    cropped.thumbnail((size, size), Image.Resampling.LANCZOS)
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    ox = (size - cropped.width) // 2
    oy = (size - cropped.height) // 2
    canvas.paste(cropped, (ox, oy), cropped)
    canvas = add_light_edge(canvas, lift=36 if size <= 24 else 22)
    return canvas


def dib_from_rgba(im: Image.Image) -> bytes:
    """32-bit BI_RGB DIB with doubled height and a 1-bit AND mask (ICO convention)."""
    w, h = im.size
    # ICO stores rows bottom-up.
    pixels = im.convert("RGBA").load()
    xor = bytearray()
    and_rows = []
    for y in range(h - 1, -1, -1):
        and_bits = []
        for x in range(w):
            r, g, b, a = pixels[x, y]
            xor += bytes((b, g, r, a))
            and_bits.append(1 if a < 128 else 0)
        # pad AND row to 32-bit boundary
        while len(and_bits) % 32:
            and_bits.append(0)
        row = bytearray()
        for i in range(0, len(and_bits), 8):
            byte = 0
            for bit in and_bits[i : i + 8]:
                byte = (byte << 1) | bit
            row.append(byte)
        and_rows.append(bytes(row))
    and_mask = b"".join(and_rows)
    header = struct.pack(
        "<IiiHHIIiiII",
        40,  # biSize
        w,
        h * 2,  # XOR + AND
        1,  # planes
        32,  # bit count
        0,  # BI_RGB
        len(xor) + len(and_mask),
        0,
        0,
        0,
        0,
    )
    return header + xor + and_mask


def png_bytes(im: Image.Image) -> bytes:
    buf = io.BytesIO()
    im.save(buf, format="PNG", optimize=True)
    return buf.getvalue()


def write_ico(images: list[Image.Image], path: Path) -> None:
    entries = []
    payloads = []
    for im in images:
        w, h = im.size
        if w >= 256 and h >= 256:
            data = png_bytes(im)
        else:
            data = dib_from_rgba(im)
        # ICO directory width/height are 1-byte; 256 is stored as 0.
        entries.append((0 if w >= 256 else w, 0 if h >= 256 else h, len(data)))
        payloads.append(data)

    offset = 6 + 16 * len(images)
    out = bytearray()
    out += struct.pack("<HHH", 0, 1, len(images))
    for (w, h, size), _ in zip(entries, payloads):
        out += struct.pack("<BBBBHHII", w, h, 0, 0, 1, 32, size, offset)
        offset += size
    for data in payloads:
        out += data
    path.write_bytes(out)


def main() -> None:
    src = Image.open(SRC).convert("RGBA")
    images = [render_size(src, s) for s in SIZES]
    PREVIEW_DIR.mkdir(parents=True, exist_ok=True)
    for im, s in zip(images, SIZES):
        im.save(PREVIEW_DIR / f"icon-{s}.png")
    write_ico(images, DST)
    print(f"wrote {DST} ({DST.stat().st_size} bytes) sizes={list(SIZES)}")


if __name__ == "__main__":
    main()
