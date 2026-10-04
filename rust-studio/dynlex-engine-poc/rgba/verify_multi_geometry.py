#!/usr/bin/env python3
"""Exact PDF→OPGM-v2→DynLex comparison for bounded overlapping fills."""
import argparse
import json
import struct
import subprocess
import tempfile
from pathlib import Path

from verify_large_rotation import display_size, pdf_rect, run_checked
from sys import path as search_path
search_path.insert(0,str(Path(__file__).resolve().parent.parent))
from verify_path_v3 import pdf_with_stream  # noqa: E402

COLORS=((255,0,0),(0,0,255),(0,255,0))

def scene(width,height,records,magic=b'OPDL',version=2):
    header=magic+struct.pack('<8I',version,width,height,0,0,width,height,0)+struct.pack('<I',len(records))
    return header+b''.join(struct.pack('<5I4B',1,*rect,*ink,255) for rect,ink in records)

def geom(media,crop,turn,records):
    return (b'OPGM'+struct.pack('<9I',2,*media,*crop,turn,len(records))
            +b''.join(struct.pack('<5I4B',1,*rect,*ink,255) for rect,ink in records))

def expected(width,height,records):
    pixels=bytearray(b'\xff\xff\xff\xff'*(width*height))
    for (left,top,right,bottom),ink in records:
        color=bytes(ink)+b'\xff'
        for y in range(top,bottom):
            for x in range(left,right):
                pos=4*(y*width+x);pixels[pos:pos+4]=color
    return pixels

def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ('probe','geometry','exporter','hayro'):
        p.add_argument('--'+name,required=True,type=Path)
    p.add_argument('--json',type=Path)
    args=p.parse_args()
    probe,geometry,exporter,hayro=(str(getattr(args,name).resolve(strict=True))
                                   for name in ('probe','geometry','exporter','hayro'))
    normal=[((25,20,95,60),COLORS[0]),((65,40,145,100),COLORS[1]),
            ((85,70,125,120),COLORS[2])]
    large=[((100,80,400,280),COLORS[0]),((280,200,650,450),COLORS[1]),
           ((350,300,550,500),COLORS[2])]
    dense=[((10+column*20,10+row*20,20+column*20,20+row*20),
            COLORS[(row+column)%len(COLORS)])
           for row in range(8) for column in range(8)]
    cases=[
        ('normal',192,256,(0,0,192,256),0,normal),
        ('quarter-90',192,256,(0,0,192,256),90,normal),
        ('quarter-180',192,256,(0,0,192,256),180,normal),
        ('quarter-270',192,256,(0,0,192,256),270,normal),
        ('crop-90',220,280,(20,30,170,230),90,normal),
        ('max-64',192,256,(0,0,192,256),90,dense),
        ('a1-90',1684,2384,(0,0,1684,2384),90,large),
    ]
    report={'geometry_version':2,'cases':{}}
    with tempfile.TemporaryDirectory(prefix='opds-opgm-v2-') as folder:
        root=Path(folder)
        for name,mw,mh,crop,turn,records in cases:
            width,height=display_size(crop,turn)
            content=bytearray()
            for rect,ink in records:
                x,y,w,h=pdf_rect(crop,turn,rect)
                rgb=' '.join(str(v//255) for v in ink)
                content.extend(f'{rgb} rg\n{x} {y} {w} {h} re\nf\n'.encode())
            pdf=root/f'{name}.pdf';opgm=root/f'{name}.opgm';opdl=root/f'{name}.opdl'
            inherited=f'/Rotate {turn} /CropBox [{" ".join(map(str,crop))}]'.encode()
            pdf.write_bytes(pdf_with_stream(content,mw,mh,inherited))
            run_checked([exporter,pdf,opgm])
            if opgm.read_bytes()!=geom((mw,mh),crop,turn,records):
                raise RuntimeError(f'{name}: wrong PDF→OPGM bytes')
            run_checked([geometry,opgm,opdl])
            if opdl.read_bytes()!=scene(width,height,records):
                raise RuntimeError(f'{name}: wrong OPGM→OPDL bytes')
            result=root/f'{name}.rgba';reference=root/f'{name}-hayro.rgba'
            run_checked([probe,'2',opdl,result,'--experimental-large'])
            run_checked([hayro,pdf,'1',reference])
            if result.read_bytes()!=reference.read_bytes() or result.read_bytes()!=expected(width,height,records):
                raise RuntimeError(f'{name}: pixel mismatch')
            report['cases'][name]={'pixels':[width,height],'rgba_bytes_exact':width*height*4,
                                   'commands':len(records)}
        bad_pdf={
            'missing-color':b'20 20 30 30 re\nf\n',
            'nonbinary-color':b'0.5 0 0 rg\n20 20 30 30 re\nf\n',
            'unpainted':b'1 0 0 rg\n20 20 30 30 re\n',
            'outside-crop':b'1 0 0 rg\n180 20 30 30 re\nf\n',
            'unsupported-clip':b'1 0 0 rg\n20 20 30 30 re\nW\nf\n',
            'too-many':(b'1 0 0 rg\n20 20 30 30 re\nf\n'*65),
        }
        for name,content in bad_pdf.items():
            pdf=root/f'bad-{name}.pdf';out=root/f'bad-{name}.opgm'
            pdf.write_bytes(pdf_with_stream(content,192,256,b'/Rotate 90'))
            rejected=subprocess.run([exporter,pdf,out],capture_output=True)
            if rejected.returncode==0 or out.exists():raise RuntimeError(f'accepted PDF {name}')
        report['unsupported_pdfs_rejected']=len(bad_pdf)
        valid=bytearray(geom((192,256),(0,0,192,256),90,normal))
        bad_opgm={
            'truncated':valid[:-1],
            'wrong-count':valid[:36]+struct.pack('<I',4)+valid[40:],
            'wrong-kind':valid[:40]+struct.pack('<I',3)+valid[44:],
            'transparent':valid[:63]+b'\x80'+valid[64:],
            'extra-byte':valid+b'\x00',
        }
        for name,data in bad_opgm.items():
            source=root/f'bad-{name}.opgm';out=root/f'bad-{name}.opdl'
            source.write_bytes(data)
            rejected=subprocess.run([geometry,source,out],capture_output=True)
            if rejected.returncode==0 or out.exists():raise RuntimeError(f'accepted OPGM {name}')
        report['invalid_geometry_records_rejected']=len(bad_opgm)
    if args.json:args.json.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))

if __name__=='__main__':main()
