#!/usr/bin/env python3
"""Research-only crossing-contour and whole original text-page pixel diagnosis."""
import argparse
import hashlib
import json
import random
import struct
import subprocess
import tempfile
from pathlib import Path


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def run(args):
    return subprocess.run(list(map(str, args)), capture_output=True, text=True, timeout=120)


def require(args):
    p = run(args)
    if p.returncode:
        raise RuntimeError(p.stderr)
    return p


def difference(a, b, width, channels):
    if len(a) != len(b):
        raise ValueError('different buffer lengths')
    delta = [(i, x, y) for i, (x, y) in enumerate(zip(a, b)) if x != y]
    return dict(bytes=len(a), different_channels=len(delta),
                different_pixels=len({i // channels for i, _, _ in delta}),
                maximum_error=max((abs(x-y) for _, x, y in delta), default=0),
                samples=[dict(x=i//channels % width, y=i//channels//width,
                              channel=i % channels, candidate=x, oracle=y) for i, x, y in delta[:32]])


def opm(contours):
    b = struct.pack('<4s4I', b'OPM2', 1, 256, 192, len(contours))
    for c in contours:
        c = c + c[:1]
        b += struct.pack('<I', len(c)) + b''.join(struct.pack('<2d', *p) for p in c)
    return b


def raster(binary, data, folder):
    p, out = folder/'input.opnw', folder/'candidate.raw'
    p.write_bytes(b'OPNW'+data[4:])
    require([binary,p,out])
    return out.read_bytes()


def dense_crossings(data, steps):
    # Independent de Casteljau polynomial sampling, diagnostic only.
    contours=[]; points=[]
    for at in range(20,len(data),40):
        opcode,flags,x,y,u,v=struct.unpack('<II4d',data[at:at+40])
        if opcode==1: points=[(x,y)]
        elif opcode==2: points.append((x,y))
        elif opcode==3:
            x0,y0=points[-1]
            for k in range(1,steps+1):
                t=k/steps; m=1-t
                points.append((m*m*x0+2*m*t*x+t*t*u,m*m*y0+2*m*t*y+t*t*v))
        elif opcode==4:
            if points[-1]!=points[0]:points.append(points[0])
            contours.append(points)
    def cross(a,b,c):return (b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0])
    found=[]
    for ci,p in enumerate(contours):
        for i,(a,b) in enumerate(zip(p,p[1:])):
            for j in range(i+2,len(p)-1):
                if i==0 and j==len(p)-2:continue
                c,d=p[j:j+2]
                if cross(a,b,c)*cross(a,b,d)<0 and cross(c,d,a)*cross(c,d,b)<0:
                    found.append(dict(contour=ci,edges=[i,j],points=[a,b,c,d]))
    return dict(steps_per_quadratic=steps,points=sum(map(len,contours)),strict_crossings=found)


def text_fixture(font, rotation, crop):
    # A real embedded-font PDF with colors, overlap, TJ and q/Q restoration.
    content=b"0 0 1 rg 60 140 510 18 re f\nq 1 0 0 rg /F 18 Tf 0.1 Tc 0.2 Tw BT 1 0 0 1 250.375 146.25 Tm [(AV oxB!gj) 30 (012345)] TJ 0 -24 TD (Tile boundaries) Tj ET Q\n0 g BT /F 12 Tf 1 0 0 1 250.125 500.25 Tm (Crossing and text) Tj ET\n"
    stream=lambda dictionary, data: b'<< '+dictionary+b' /Length '+str(len(data)).encode()+b' >>\nstream\n'+data+b'\nendstream'
    objects=[b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        f'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 640 800] /CropBox [{crop}] /Rotate {rotation} /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>'.encode(),
        stream(b'',content),
        b'<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSans /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths ['+b'600 '*95+b'] /FontDescriptor 6 0 R >>',
        b'<< /Type /FontDescriptor /FontName /LiberationSans /Flags 32 /FontBBox [-543 -303 1301 980] /ItalicAngle 0 /Ascent 905 /Descent -212 /CapHeight 687 /StemV 80 /FontFile2 7 0 R >>',
        stream(b'/Length1 '+str(len(font)).encode(),font)]
    data=bytearray(b'%PDF-1.7\n');offsets=[0]
    for i,obj in enumerate(objects,1):
        offsets.append(len(data));data.extend(f'{i} 0 obj\n'.encode()+obj+b'\nendobj\n')
    xref=len(data);data.extend(f'xref\n0 {len(objects)+1}\n0000000000 65535 f \n'.encode())
    for off in offsets[1:]:data.extend(f'{off:010} 00000 n \n'.encode())
    data.extend(f'trailer << /Root 1 0 R /Size {len(objects)+1} >>\nstartxref\n{xref}\n%%EOF\n'.encode())
    return bytes(data)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir',type=Path,default=Path('target/release'))
    parser.add_argument('--manifest',type=Path,default=Path('docs/public-pdf-corpus-2026-10-08.json'))
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--require-exact',action='store_true')
    args=parser.parse_args()
    names=['winding_probe','truetype_page_probe','glyph_vello_probe','quad_flatten_oracle','backend_probe']
    bins={n: (args.bin_dir/n).resolve(strict=True) for n in names}
    hashes={n:sha(p) for n,p in bins.items()}
    source=next(r for r in json.loads(args.manifest.read_text()) if r['id']=='albany-contract')
    pdf=Path(source['path'])
    assert sha(pdf)==source['sha256']
    report=dict(purpose='explicitly experimental, no native acceptance',source_sha256=sha(pdf),
                binary_sha256=hashes,synthetic=[],whole_page=[])
    rng=random.Random(8102026)
    with tempfile.TemporaryDirectory(prefix='opds-winding-audit-') as tmp:
        folder=Path(tmp)
        for case in range(48):
            x,y=rng.randrange(4,160)+0.25,rng.randrange(4,100)+0.5
            w,h=rng.randrange(5,60),rng.randrange(5,60)
            rect=[(x,y),(x+w,y),(x+w,y+h),(x,y+h)]
            bow=[rect[0],rect[2],rect[3],rect[1]]
            other=[(px+3,py+4) for px,py in rect]
            contours=[bow] if case%4==0 else [rect,other] if case%4==1 else [rect,list(reversed(other))] if case%4==2 else [bow,list(reversed(rect))]
            data=opm(contours)
            (folder/'input.opm').write_bytes(data)
            require([bins['glyph_vello_probe'],'--opm',folder/'input.opm',folder/'reference.raw'])
            report['synthetic'].append(dict(case=case,**difference(raster(bins['winding_probe'],data,folder),(folder/'reference.raw').read_bytes(),256,1)))
        for scale in [0.25,1,2]:
            stage=folder/str(scale);stage.mkdir()
            output=stage/'page.rgba'
            result=run([bins['truetype_page_probe'],pdf,6,scale,output,'--experimental-winding',stage])
            row=dict(scale=scale,exit=result.returncode,diagnostic=result.stderr.strip(),rgba_written=output.exists())
            if result.returncode==0:
                require([bins['backend_probe'],pdf,stage/'reference.rgba','--hayro',f'--scale={scale}','--page=6'])
                row['pixels']=difference(output.read_bytes(),(stage/'reference.rgba').read_bytes(),int(612*scale),4)
            elif output.exists():
                raise RuntimeError('partial output on rejection')
            raw=stage/'paint-8.opq'
            if raw.exists():
                require([bins['quad_flatten_oracle'],raw,stage/'flat.txt',stage/'oracle.opm',stage/'quad.raw'])
                expected=(stage/'quad.raw').read_bytes()
                row['apostrophe_oracle_flat']=difference(raster(bins['winding_probe'],(stage/'oracle.opm').read_bytes(),folder),expected,256,1)
                row['apostrophe_own_flat']=difference(raster(bins['winding_probe'],(stage/'paint-8.center.opm').read_bytes(),folder),expected,256,1)
                row['quadratic_sha256']=sha(raw)
                row['dense_original_outline']=[dense_crossings(raw.read_bytes(),n) for n in [64,128]]
            report['whole_page'].append(row)
        report['text_fixtures']=[]
        font=Path('assets/fonts/LiberationSans-Regular.ttf').read_bytes()
        report['fixture_font_sha256']=hashlib.sha256(font).hexdigest()
        for rotation in [0,90,180,270]:
            for crop in ['0 0 640 800','8 8 632 792']:
                root=folder/f'fixture-{rotation}-{crop[0]}';root.mkdir()
                pdf_case=root/'source.pdf';pdf_case.write_bytes(text_fixture(font,rotation,crop))
                result=run([bins['truetype_page_probe'],pdf_case,1,1,root/'candidate.rgba','--experimental-winding',root])
                row=dict(rotation=rotation,crop=crop,source_sha256=sha(pdf_case),exit=result.returncode,diagnostic=result.stderr.strip())
                if result.returncode==0:
                    require([bins['backend_probe'],pdf_case,root/'oracle.rgba','--hayro','--scale=1'])
                    dim=list(map(int,(root/'staged.tsv').read_text().splitlines()[0].split('\t')[1:3]))
                    row['pixels']=difference((root/'candidate.rgba').read_bytes(),(root/'oracle.rgba').read_bytes(),dim[0],4)
                elif (root/'candidate.rgba').exists():raise RuntimeError('partial fixture output')
                report['text_fixtures'].append(row)
        # Invalid input never creates an output. Cover framing and geometry.
        data=opm([[(10.,10.),(20.,20.),(10.,20.),(20.,10.)]])
        data=b'OPNW'+data[4:]
        invalid=[data[:19],data[:-1],data+b'X',b'OPM2'+data[4:],data[:24]+struct.pack('<d',float('nan'))+data[32:]]
        for i,b in enumerate(invalid):
            p,out=folder/f'bad{i}.opnw',folder/f'bad{i}.raw';p.write_bytes(b)
            result=run([bins['winding_probe'],p,out])
            if result.returncode==0 or out.exists(): raise RuntimeError('invalid OPNW accepted')
        report['invalid_cli_rejected']=len(invalid)
    if hashes!={n:sha(p) for n,p in bins.items()}:raise RuntimeError('binary changed')
    report['synthetic_all_exact']=all(r['different_channels']==0 for r in report['synthetic'])
    report['all_exact'] = report['synthetic_all_exact'] and all(r.get('pixels',{}).get('different_channels') == 0 for r in report['whole_page']+report['text_fixtures'])
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    if args.require_exact and not report['all_exact']:raise SystemExit('pixel mismatch or rejected case; diagnosis saved')
    print(json.dumps({k:v for k,v in report.items() if k in ('synthetic_all_exact','whole_page','text_fixtures','invalid_cli_rejected')},indent=2))


if __name__=='__main__': main()
