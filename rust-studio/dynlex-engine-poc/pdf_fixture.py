#!/usr/bin/env python3
"""Generate three small, valid PDFs with real RGB image and filled path objects."""

from pathlib import Path

WIDTH, HEIGHT = 256, 192
RECTS = ((25, 20, 190, 24, (255, 0, 0)),
         (50, 45, 62, 110, (0, 0, 255)),
         (90, 75, 220, 130, (255, 255, 0)))


def image_rgb():
    return bytes(channel for y in range(HEIGHT) for x in range(WIDTH)
                 for channel in ((x * 3 + y) % 256, (y * 5 + 37) % 256, (x ^ y) % 256))


def build_pdf(mode, extra_content=b""):
    content = bytearray()
    if mode != "vector":
        content.extend(b"q\n256 0 0 192 0 0 cm\n/Im0 Do\nQ\n")
    if mode != "raster":
        for x0, y0, x1, y1, (r, g, b) in RECTS:
            content.extend(f"{r / 255:g} {g / 255:g} {b / 255:g} rg\n"
                           f"{x0} {HEIGHT - y1} {x1 - x0} {y1 - y0} re\nf\n".encode())
    content.extend(extra_content)
    rgb = image_rgb()
    has_image = mode != "vector"
    content_id = 5 if has_image else 4
    resources = "/Resources << /XObject << /Im0 4 0 R >> >>" if has_image else "/Resources << >>"
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {WIDTH} {HEIGHT}] "
        f"{resources} /Contents {content_id} 0 R >>".encode(),
    ]
    if has_image:
        objects.append(f"<< /Type /XObject /Subtype /Image /Width {WIDTH} /Height {HEIGHT} "
                       f"/ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {len(rgb)} >>\nstream\n".encode()
                       + rgb + b"\nendstream")
    objects.append(f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"endstream")
    pdf = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = [0]
    for index, obj in enumerate(objects, 1):
        offsets.append(len(pdf))
        pdf.extend(f"{index} 0 obj\n".encode() + obj + b"\nendobj\n")
    xref = len(pdf)
    pdf.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        pdf.extend(f"{offset:010d} 00000 n \n".encode())
    pdf.extend(f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
    return bytes(pdf)


def reference_rgba(mode):
    rgba = bytearray()
    for y in range(HEIGHT):
        for x in range(WIDTH):
            color = (255, 255, 255) if mode == "vector" else (
                (x * 3 + y) % 256, (y * 5 + 37) % 256, (x ^ y) % 256)
            if mode != "raster":
                for x0, y0, x1, y1, ink in RECTS:
                    if x0 <= x < x1 and y0 <= y < y1:
                        color = ink
            rgba.extend((*color, 255))
    return bytes(rgba)


if __name__ == "__main__":
    import sys
    directory = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("/tmp/opds-dynlex-pdf-fixtures")
    directory.mkdir(parents=True, exist_ok=True)
    for mode in ("raster", "vector", "hybrid"):
        (directory / f"{mode}.pdf").write_bytes(build_pdf(mode))
        (directory / f"{mode}.rgba").write_bytes(reference_rgba(mode))
        print(directory / f"{mode}.pdf")
