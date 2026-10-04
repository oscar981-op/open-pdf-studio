#!/usr/bin/env python3
"""Verify bounded experimental OPDL-v2 vector pages and quarter-turn geometry.

The geometry mapping is an independent test harness, not a PDF adapter.
Only one opaque axis-aligned rectangle is used; no general PDF claim follows.
"""
import argparse
import json
import struct
import subprocess
import tempfile
from pathlib import Path
from sys import path as search_path

search_path.insert(0, str(Path(__file__).resolve().parent.parent))
from verify_path_v3 import pdf_with_stream  # noqa: E402

GEOMETRY_VERSION = 1
MAX_SIDE = 4096
MAX_PIXELS = 6_000_000
RECT = (25, 20, 90, 45)


def display_size(crop, turn):
    x0, y0, x1, y1 = crop
    return (y1-y0, x1-x0) if turn in (90, 270) else (x1-x0, y1-y0)


def pdf_rect(crop, turn, rect):
    x0, y0, x1, y1 = crop
    left, top, right, bottom = rect
    if turn == 0:
        return (x0+left, y1-bottom, right-left, bottom-top)
    if turn == 90:
        return (x0+top, y0+left, bottom-top, right-left)
    if turn == 180:
        return (x1-right, y0+top, right-left, bottom-top)
    if turn == 270:
        return (x1-bottom, y1-right, bottom-top, right-left)
    raise ValueError('unsupported rotation')


def scene(width, height, rect, image_len=0):
    if width < 1 or height < 1 or width > MAX_SIDE or height > MAX_SIDE or width*height > MAX_PIXELS:
        raise ValueError('page outside experimental pixel bound')
    left, top, right, bottom = rect
    if not (0 <= left < right <= width and 0 <= top < bottom <= height):
        raise ValueError('rectangle outside page')
    return (b'OPDL' + struct.pack('<8I', 2, width, height, 0, 0, width, height, image_len)
            + struct.pack('<I', 1) + struct.pack('<5I4B', 1, left, top, right, bottom, 255, 0, 0, 255))


def expected(width, height, rect):
    pixels = bytearray(b'\xff\xff\xff\xff' * (width*height))
    left, top, right, bottom = rect
    for y in range(top, bottom):
        for x in range(left, right):
            offset = (y*width+x)*4
            pixels[offset:offset+4] = b'\xff\x00\x00\xff'
    return pixels


def run_checked(command):
    return subprocess.run(command, check=True, capture_output=True, text=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--probe', required=True, type=Path)
    parser.add_argument('--geometry', required=True, type=Path)
    parser.add_argument('--exporter', required=True, type=Path)
    parser.add_argument('--hayro', required=True, type=Path)
    parser.add_argument('--json', type=Path)
    args = parser.parse_args()
    probe, geometry, exporter, hayro = (str(path.resolve(strict=True)) for path in (args.probe,args.geometry,args.exporter,args.hayro))
    cases = [
        ('quarter-0', 192, 256, (0,0,192,256), 0, RECT, False),
        ('quarter-90', 192, 256, (0,0,192,256), 90, RECT, False),
        ('quarter-180', 192, 256, (0,0,192,256), 180, RECT, False),
        ('quarter-270', 192, 256, (0,0,192,256), 270, RECT, False),
        ('shifted-crop-90', 220, 280, (20,30,170,230), 90, RECT, False),
        ('inherited-media-90', 192, 256, (0,0,192,256), 90, RECT, True),
        ('a1-quarter-90', 1684, 2384, (0,0,1684,2384), 90, (100,80,260,130), False),
    ]
    report={'geometry_version':GEOMETRY_VERSION,'experimental_pixel_limit':MAX_PIXELS,'cases':{}}
    with tempfile.TemporaryDirectory(prefix='opds-large-rotate-') as directory:
        work=Path(directory)
        for name,media_w,media_h,crop,turn,rect,inherited_media in cases:
            width,height=display_size(crop,turn)
            px,py,pw,ph=pdf_rect(crop,turn,rect)
            inherited=f'/Rotate {turn} /CropBox [{" ".join(map(str,crop))}]'.encode()
            if inherited_media:
                inherited+=f' /MediaBox [0 0 {media_w} {media_h}]'.encode()
            pdf=work/f'{name}.pdf';opdl=work/f'{name}.opdl';geom=work/f'{name}.opgm'
            pdf_bytes=pdf_with_stream(f'1 0 0 rg\n{px} {py} {pw} {ph} re\nf\n'.encode(),
                                      media_w,media_h,inherited)
            if inherited_media:
                needle=f'/MediaBox [0 0 {media_w} {media_h}]'.encode()
                page_start=pdf_bytes.index(b'3 0 obj\n')
                at=pdf_bytes.index(needle,page_start)
                pdf_bytes=pdf_bytes[:at]+b' '*len(needle)+pdf_bytes[at+len(needle):]
            pdf.write_bytes(pdf_bytes)
            run_checked([exporter,pdf,geom])
            expected_geom=(b'OPGM'+struct.pack('<12I',GEOMETRY_VERSION,media_w,media_h,*crop,turn,*rect)
                           +bytes((255,0,0,255)))
            if geom.read_bytes()!=expected_geom:
                raise RuntimeError(f'{name}: PDF exporter differs from independent geometry')
            mapped=run_checked([geometry,geom,opdl]).stdout.strip()
            if mapped!=f'{px} {py} {pw} {ph}' or opdl.read_bytes()!=scene(width,height,rect):
                raise RuntimeError(f'{name}: OPGM geometry differs from independent mapping')
            actual=work/f'{name}.rgba';reference=work/f'{name}-hayro.rgba'
            run_checked([probe,'2',opdl,actual,'--experimental-large'])
            run_checked([hayro,pdf,'1',reference])
            if actual.read_bytes()!=reference.read_bytes() or actual.read_bytes()!=expected(width,height,rect):
                raise RuntimeError(f'{name}: pixel mismatch')
            if width>1024 or height>1024:
                rejected=subprocess.run([probe,'2',opdl,work/'legacy.rgba'],capture_output=True)
                if rejected.returncode==0 or (work/'legacy.rgba').exists():
                    raise RuntimeError('ordinary probe accepted experimental large scene')
            report['cases'][name]={'display_pixels':[width,height],'pixel_bytes_exact':width*height*4}
        invalid={
            'side-oversize':(4097,1,0),
            'pixel-oversize':(3000,3000,0),
            'large-rgb':(2384,1684,1),
        }
        for name,(width,height,image_len) in invalid.items():
            if name!='large-rgb':
                # Directly craft a corrupt OPDL header to test Rust validation.
                bad=bytearray(scene(256,192,RECT))
                struct.pack_into('<II',bad,8,width,height)
            else:
                bad=bytearray(scene(width,height,RECT,image_len))
            path=work/f'{name}.opdl';out=work/f'{name}.rgba'
            path.write_bytes(bad)
            rejected=subprocess.run([probe,'2',path,out,'--experimental-large'],capture_output=True)
            if rejected.returncode==0 or out.exists():raise RuntimeError(f'accepted {name}')
        report['invalid_scenes_rejected']=len(invalid)
        large_scene=work/'a1-quarter-90.opdl'
        large_output=work/'invalid-large-mode.rgba'
        for mode in ('1','3'):
            rejected=subprocess.run([probe,mode,large_scene,large_output,'--experimental-large'],capture_output=True)
            if rejected.returncode==0 or large_output.exists():raise RuntimeError('accepted non-vector large page')
        report['invalid_large_modes_rejected']=2
        valid=bytearray(b'OPGM'+struct.pack('<12I',GEOMETRY_VERSION,192,256,0,0,192,256,90,*RECT)
                        +bytes((255,0,0,255)))
        bad_records={
            'truncated':valid[:-1],
            'bad-version':valid[:4]+struct.pack('<I',2)+valid[8:],
            'bad-rotation':valid[:32]+struct.pack('<I',45)+valid[36:],
            'crop-outside-media':valid[:24]+struct.pack('<I',300)+valid[28:],
            'nonopaque-ink':valid[:-1]+b'\x00',
        }
        for name,data in bad_records.items():
            source=work/f'bad-{name}.opgm';out=work/f'bad-{name}.opdl'
            source.write_bytes(data)
            rejected=subprocess.run([geometry,source,out],capture_output=True)
            if rejected.returncode==0 or out.exists():raise RuntimeError(f'accepted OPGM {name}')
        report['invalid_geometry_records_rejected']=len(bad_records)
        invalid_pdfs={
            'missing-fill':(b'1 0 0 rg\n20 25 25 65 re\n',b'/Rotate 90'),
            'two-fills':(b'1 0 0 rg\n20 25 25 65 re\nf\n20 25 2 2 re\nf\n',b'/Rotate 90'),
            'unsupported-color':(b'0 0 1 rg\n20 25 25 65 re\nf\n',b'/Rotate 90'),
            'bad-rotation':(b'1 0 0 rg\n20 25 25 65 re\nf\n',b'/Rotate 45'),
            'crop-outside':(b'1 0 0 rg\n20 25 25 65 re\nf\n',b'/CropBox [0 0 300 256]'),
            'rectangle-outside-crop':(b'1 0 0 rg\n20 25 25 65 re\nf\n',b'/CropBox [40 0 180 256]'),
            'unknown-operator':(b'1 0 0 rg\n20 25 25 65 re\nf\nq\n',b'/Rotate 90'),
            'fractional-rectangle':(b'1 0 0 rg\n20.5 25 25 65 re\nf\n',b'/Rotate 90'),
            'compound-operand':(b'(ignored) Tj\n1 0 0 rg\n20 25 25 65 re\nf\n',b'/Rotate 90'),
        }
        for name,(content,inherited) in invalid_pdfs.items():
            source=work/f'bad-pdf-{name}.pdf';out=work/f'bad-pdf-{name}.opgm'
            source.write_bytes(pdf_with_stream(content,192,256,inherited))
            rejected=subprocess.run([exporter,source,out],capture_output=True)
            if rejected.returncode==0 or out.exists():raise RuntimeError(f'accepted PDF {name}')
        report['unsupported_pdfs_rejected']=len(invalid_pdfs)
    if args.json:args.json.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))

if __name__=='__main__':main()
