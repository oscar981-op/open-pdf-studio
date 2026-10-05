#!/usr/bin/env python3
"""Generate three small, valid PDFs with real RGB image and filled path objects."""

from pathlib import Path
import zlib

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


def png_predictor_rgb(rgb, width, height):
    """Encode rows with PNG filters 0..4 in rotation for PDF Predictor 15."""
    row_bytes = width * 3
    encoded = bytearray()
    previous = bytes(row_bytes)
    for y in range(height):
        row = rgb[y * row_bytes:(y + 1) * row_bytes]
        filter_kind = y % 5
        encoded.append(filter_kind)
        for i, value in enumerate(row):
            left = row[i - 3] if i >= 3 else 0
            up = previous[i]
            upper_left = previous[i - 3] if i >= 3 else 0
            if filter_kind == 0:
                predicted = 0
            elif filter_kind == 1:
                predicted = left
            elif filter_kind == 2:
                predicted = up
            elif filter_kind == 3:
                predicted = (left + up) // 2
            else:
                p = left + up - upper_left
                differences = (abs(p - left), abs(p - up), abs(p - upper_left))
                predicted = (left, up, upper_left)[differences.index(min(differences))]
            encoded.append((value - predicted) & 255)
        previous = row
    return bytes(encoded)


def build_many_images_pdf(image_count=12, compress_alternate=False):
    """A controlled layered PDF with distinct XObject names and vector fills."""
    if not 1 <= image_count <= 257:
        raise ValueError("image_count must be 1..257")
    content = bytearray()
    image_objects = []
    resources = []
    for index in range(image_count):
        if index in (2, 8):
            content.extend(f"1 0 0 rg\n{20 + index} 58 48 30 re\nf\n".encode())
        name = f"Picture-{index:03}"
        x, y = 9 + (index * 13) % 175, 11 + (index * 7) % 135
        w, h = 48, 32
        content.extend(f"q\n{w} 0 0 {h} {x} {y} cm\n/{name} Do\nQ\n".encode())
        rgb = bytes((channel + index * 19) % 256 for channel in image_rgb(w, h))
        compressed = compress_alternate and index % 2 == 1
        payload = zlib.compress(rgb) if compressed else rgb
        filter_entry = "/Filter /FlateDecode" if compressed else ""
        image_objects.append(
            f"<< /Type /XObject /Subtype /Image /Width {w} /Height {h} "
            f"/ColorSpace /DeviceRGB /BitsPerComponent 8 {filter_entry} "
            f"/Length {len(payload)} >>\nstream\n".encode()
            + payload + b"\nendstream"
        )
        resources.append(f"/{name} {4 + index} 0 R")
    content_id = 4 + image_count
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
        (f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {WIDTH} {HEIGHT}] "
         f"/Resources << /XObject << {' '.join(resources)} >> >> "
         f"/Contents {content_id} 0 R >>").encode(),
        *image_objects,
        f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"endstream",
    ]
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


def build_pdf(mode, extra_content=b"", width=WIDTH, height=HEIGHT, dense=False, translated=False, clipped=False, pre_content=b"", clip_case=None, diagonal=False, diagonal_color=(0, 0, 0), falling=False, many_rects=False, multi_segment=False, vertical_multi=False, pages_extra="", image_extra="", flate_image=False, image_bytes_extra=b"", image_placement=None, second_image_placement=None, second_image_after_vector=False, image_source_size=None, image_source_override=None, png_predictor=False, indexed_palette=None, indexed_pixels=None):
    content = bytearray(pre_content)
    if mode != "vector":
        image_x, image_y, image_width, image_height = image_placement or (0, 0, width, height)
        content.extend(f"q\n{image_width} 0 0 {image_height} {image_x} {image_y} cm\n/Im0 Do\nQ\n".encode())
        if second_image_placement and not second_image_after_vector:
            x, y, w, h = second_image_placement
            content.extend(f"q\n{w} 0 0 {h} {x} {y} cm\n/Im1 Do\nQ\n".encode())
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
    if many_rects and mode != "raster":
        content.extend(b"1 0 0 rg\n")
        for _ in range(60):
            for row in range(10):
                for column in range(10):
                    x, y = 10 + 24 * column, 10 + 16 * row
                    content.extend(f"{x} {height - y - 4} 4 4 re\nf\n".encode())
    if multi_segment and mode != "raster":
        if vertical_multi:
            endpoints = f"20.5 {height - 32} m\n20.5 {height - 72} l\n20.5 {height - 112} l"
        else:
            pdf_y = height - 40.5
            endpoints = f"20 {pdf_y:g} m\n60 {pdf_y:g} l\n100 {pdf_y:g} l"
        content.extend(f"1 w\n0 0 0 RG\n{endpoints}\nS\n".encode())
    if second_image_placement and second_image_after_vector:
        x, y, w, h = second_image_placement
        content.extend(f"q\n{w} 0 0 {h} {x} {y} cm\n/Im1 Do\nQ\n".encode())
    content.extend(extra_content)
    source_width, source_height = (image_source_size or (image_width, image_height)) if mode != "vector" else (0, 0)
    indexed = indexed_palette is not None
    if indexed:
        if mode == "vector" or second_image_placement or image_source_override is not None or png_predictor:
            raise ValueError("Indexed fixture supports one image without PNG predictor")
        if not 3 <= len(indexed_palette) <= 768 or len(indexed_palette) % 3:
            raise ValueError("Indexed palette must contain 1..256 RGB colors")
        if indexed_pixels is None or len(indexed_pixels) != source_width * source_height:
            raise ValueError("Indexed fixture has the wrong sample byte length")
        rgb = indexed_pixels
        color_space = (f"/ColorSpace [/Indexed /DeviceRGB {len(indexed_palette) // 3 - 1} "
                       f"<{indexed_palette.hex()}>]")
    else:
        rgb = (image_source_override if image_source_override is not None
               else image_rgb(source_width, source_height)) if mode != "vector" else b""
        if mode != "vector" and len(rgb) != source_width * source_height * 3:
            raise ValueError("image_source_override has the wrong RGB8 byte length")
        color_space = "/ColorSpace /DeviceRGB"
    if png_predictor and (not flate_image or second_image_placement):
        raise ValueError("PNG predictor fixture requires one Flate image")
    encoded_rgb = png_predictor_rgb(rgb, source_width, source_height) if png_predictor else rgb
    image_payload = zlib.compress(encoded_rgb + image_bytes_extra) if flate_image else rgb
    image_filter = "/Filter /FlateDecode" if flate_image else ""
    if png_predictor:
        image_filter += f" /DecodeParms << /Predictor 15 /Columns {source_width} /Colors 3 >>"
    has_image = mode != "vector"
    content_id = 6 if second_image_placement else (5 if has_image else 4)
    resources = ("/Resources << /XObject << /Im0 4 0 R /Im1 5 0 R >> >>"
                 if second_image_placement else
                 "/Resources << /XObject << /Im0 4 0 R >> >>" if has_image else
                 "/Resources << >>")
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        f"<< /Type /Pages /Count 1 /Kids [3 0 R] {pages_extra} >>".encode(),
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] "
        f"{resources} /Contents {content_id} 0 R >>".encode(),
    ]
    if has_image:
        objects.append(f"<< /Type /XObject /Subtype /Image /Width {source_width} /Height {source_height} "
                       f"{color_space} /BitsPerComponent 8 {image_filter} {image_extra} /Length {len(image_payload)} >>\nstream\n".encode()
                       + image_payload + b"\nendstream")
    if second_image_placement:
        _, _, second_width, second_height = second_image_placement
        second_rgb = bytes(255 - value for value in image_rgb(second_width, second_height))
        second_payload = zlib.compress(second_rgb) if flate_image else second_rgb
        objects.append(f"<< /Type /XObject /Subtype /Image /Width {second_width} /Height {second_height} "
                       f"/ColorSpace /DeviceRGB /BitsPerComponent 8 {image_filter} /Length {len(second_payload)} >>\nstream\n".encode()
                       + second_payload + b"\nendstream")
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


def reference_rgba(mode, width=WIDTH, height=HEIGHT, dense=False, translated=False, clipped=False, clip_case=None, diagonal=False, diagonal_color=(0, 0, 0), falling=False, many_rects=False, multi_segment=False, vertical_multi=False):
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
                if many_rects:
                    grid_x, grid_y = x - 10, y - 10
                    if (0 <= grid_x < 240 and 0 <= grid_y < 160
                            and grid_x % 24 < 4 and grid_y % 16 < 4):
                        color = (255, 0, 0)
                if multi_segment and ((not vertical_multi and y == 40 and 20 <= x < 100)
                                      or (vertical_multi and x == 20 and 32 <= y < 112)):
                    color = (0, 0, 0)
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
