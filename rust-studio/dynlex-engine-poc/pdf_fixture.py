#!/usr/bin/env python3
"""Generate three small, valid PDFs with real RGB image and filled path objects."""

from pathlib import Path

WIDTH, HEIGHT = 256, 192
RECTS = ((25, 20, 190, 24, (255, 0, 0)),
         (50, 45, 62, 110, (0, 0, 255)),
         (90, 75, 220, 130, (255, 255, 0)))
LINES = ((32, 32, 224, 32, (0, 255, 255)),
         (180, 18, 180, 170, (255, 0, 255)))


def rectangles(width, height, dense=False):
    if width % WIDTH or height % HEIGHT:
        raise ValueError("fixture sizes must be integer multiples of 256x192")
    sx, sy = width // WIDTH, height // HEIGHT
    base = RECTS
    if dense:
        inks = ((255, 0, 0), (0, 0, 255), (255, 255, 0), (0, 255, 0))
        base = tuple((8 + col * 30, 8 + row * 29, 27 + col * 30, 27 + row * 29,
                      inks[(row + col) % len(inks)])
                     for row in range(6) for col in range(8))
    return tuple((x0 * sx, y0 * sy, x1 * sx, y1 * sy, ink)
                 for x0, y0, x1, y1, ink in base)


def lines(width, height):
    if width % WIDTH or height % HEIGHT:
        raise ValueError("fixture sizes must be integer multiples of 256x192")
    sx, sy = width // WIDTH, height // HEIGHT
    return tuple((x0 * sx, y0 * sy, x1 * sx, y1 * sy, ink)
                 for x0, y0, x1, y1, ink in LINES)


def image_rgb(width, height):
    return bytes(channel for y in range(height) for x in range(width)
                 for channel in ((x * 3 + y) % 256, (y * 5 + 37) % 256, (x ^ y) % 256))


def build_pdf(mode, extra_content=b"", width=WIDTH, height=HEIGHT, dense=False, translated=False, clipped=False, pre_content=b"", clip_case=None, diagonal=False, diagonal_color=(0, 0, 0), falling=False):
    content = bytearray(pre_content)
    if mode != "vector":
        content.extend(f"q\n{width} 0 0 {height} 0 0 cm\n/Im0 Do\nQ\n".encode())
    if mode != "raster":
        for x0, y0, x1, y1, (r, g, b) in rectangles(width, height, dense):
            content.extend(f"{r / 255:g} {g / 255:g} {b / 255:g} rg\n"
                           f"{x0} {height - y1} {x1 - x0} {y1 - y0} re\nf\n".encode())
        content.extend(b"1 w\n")
        for x0, y0, x1, y1, (r, g, b) in lines(width, height):
            content.extend(f"{r / 255:g} {g / 255:g} {b / 255:g} RG\n".encode())
            if y0 == y1:
                pdf_y = height - y0 - 0.5
                content.extend(f"{x0} {pdf_y:g} m\n{x1} {pdf_y:g} l\nS\n".encode())
            else:
                pdf_x = x0 + 0.5
                content.extend(f"{pdf_x:g} {height - y0} m\n{pdf_x:g} {height - y1} l\nS\n".encode())
    if translated and mode != "raster":
        content.extend(f"0 1 0 rg\nq\n1 0 0 1 10 7 cm\n20 {height - 50} 50 30 re\nf\nQ\n".encode())
        content.extend(f"1 0 1 RG\nq\n1 0 0 1 10 7 cm\n"
                       f"20 {height - 60 - 0.5:g} m\n100 {height - 60 - 0.5:g} l\nS\n"
                       f"110.5 {height - 70} m\n110.5 {height - 120} l\nS\nQ\n".encode())
    if clipped and mode != "raster":
        content.extend(f"40 {height - 90} 60 50 re\nW\nn\n"
                       f"1 0.50196078 0 rg\n20 {height - 70} 100 45 re\nf\n"
                       f"0 0 0 RG\n20 {height - 60 - 0.5:g} m\n130 {height - 60 - 0.5:g} l\nS\n"
                       f"80.5 {height - 20} m\n80.5 {height - 110} l\nS\n".encode())
    if clip_case and mode != "raster":
        if clip_case == "double":
            content.extend(f"70 {height - 100} 60 50 re\nW\nn\n"
                           f"1 0 0 rg\n60 {height - 85} 60 40 re\nf\n"
                           f"1 1 1 RG\n75.5 {height - 45} m\n75.5 {height - 110} l\nS\n".encode())
        elif clip_case == "empty":
            content.extend(f"150 {height - 160} 40 40 re\nW\nn\n"
                           f"1 0 0 rg\n10 {height - 90} 200 80 re\nf\n".encode())
        elif clip_case == "restore":
            content.extend(f"1 0 0 rg\nq\n70 {height - 80} 20 20 re\nW\nn\n"
                           f"50 {height - 90} 50 40 re\nf\nQ\n"
                           f"0 0 1 rg\n50 {height - 100} 50 20 re\nf\n".encode())
        elif clip_case == "nested":
            content.extend(f"1 0 0 rg\nq\n60 {height - 90} 40 40 re\nW\nn\n"
                           f"q\n70 {height - 80} 20 20 re\nW\nn\n"
                           f"50 {height - 90} 50 40 re\nf\nQ\n"
                           f"50 {height - 100} 50 20 re\nf\nQ\n"
                           f"0 0 1 rg\n40 {height - 70} 20 20 re\nf\n".encode())
        else:
            raise ValueError(f"unknown clip case: {clip_case}")
    if diagonal and mode != "raster":
        r, g, b = diagonal_color
        if falling:
            endpoints = f"20.5 {height - 100.5:g} m\n80.5 {height - 40.5:g} l"
        else:
            endpoints = f"20.5 {height - 40.5:g} m\n80.5 {height - 100.5:g} l"
        content.extend(f"1 w\n{r / 255:g} {g / 255:g} {b / 255:g} RG\n{endpoints}\nS\n".encode())
    content.extend(extra_content)
    rgb = image_rgb(width, height) if mode != "vector" else b""
    has_image = mode != "vector"
    content_id = 5 if has_image else 4
    resources = "/Resources << /XObject << /Im0 4 0 R >> >>" if has_image else "/Resources << >>"
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] "
        f"{resources} /Contents {content_id} 0 R >>".encode(),
    ]
    if has_image:
        objects.append(f"<< /Type /XObject /Subtype /Image /Width {width} /Height {height} "
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


def reference_rgba(mode, width=WIDTH, height=HEIGHT, dense=False, translated=False, clipped=False, clip_case=None, diagonal=False, diagonal_color=(0, 0, 0), falling=False):
    rgba = bytearray()
    rects = rectangles(width, height, dense)
    strokes = lines(width, height)
    for y in range(height):
        for x in range(width):
            color = (255, 255, 255) if mode == "vector" else (
                (x * 3 + y) % 256, (y * 5 + 37) % 256, (x ^ y) % 256)
            if mode != "raster":
                for x0, y0, x1, y1, ink in rects:
                    if x0 <= x < x1 and y0 <= y < y1:
                        color = ink
                for x0, y0, x1, y1, ink in strokes:
                    if (y0 == y1 and x0 <= x < x1 and y == y0) or (x0 == x1 and x == x0 and y0 <= y < y1):
                        color = ink
                if translated:
                    if 30 <= x < 80 and 13 <= y < 43:
                        color = (0, 255, 0)
                    if (30 <= x < 110 and y == 53) or (x == 120 and 63 <= y < 113):
                        color = (255, 0, 255)
                if clipped:
                    if 40 <= x < 100 and 40 <= y < 70:
                        color = (255, 128, 0)
                    if (40 <= x < 100 and y == 60) or (x == 80 and 40 <= y < 90):
                        color = (0, 0, 0)
                    if clip_case == "double":
                        if 70 <= x < 100 and 50 <= y < 85:
                            color = (255, 0, 0)
                        if x == 75 and 50 <= y < 90:
                            color = (255, 255, 255)
                    if clip_case == "restore":
                        if 70 <= x < 90 and 60 <= y < 80:
                            color = (255, 0, 0)
                        if 50 <= x < 100 and 80 <= y < 90:
                            color = (0, 0, 255)
                    if clip_case == "nested":
                        if (70 <= x < 90 and 60 <= y < 80) or (60 <= x < 100 and 80 <= y < 90):
                            color = (255, 0, 0)
                        if 40 <= x < 60 and 50 <= y < 70:
                            color = (0, 0, 255)
                if diagonal:
                    row = y - 40
                    line_x = (80 - row) if falling else (20 + row)
                    lane = x - line_x
                    coverage = 0
                    if 0 <= row <= 60:
                        if row in (0, 60):
                            if lane == 0:
                                coverage = 117
                            elif (row == 0 and lane == (-1 if falling else 1)) or (row == 60 and lane == (1 if falling else -1)):
                                coverage = 64
                        elif lane == 0:
                            coverage = 233
                        elif abs(lane) == 1:
                            coverage = 64
                    if coverage:
                        color = tuple((ink * coverage + channel * (255 - coverage) + 255) // 256
                                      for ink, channel in zip(diagonal_color, color))
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
