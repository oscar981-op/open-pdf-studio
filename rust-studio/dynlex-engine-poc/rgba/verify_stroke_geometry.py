#!/usr/bin/env python3
"""Bounded PDF→OPGM-v3→DynLex pixel comparison; experimental only."""
import json, struct, subprocess, tempfile
from pathlib import Path
from verify_multi_geometry import COLORS
from verify_large_rotation import display_size, pdf_rect, run_checked
from sys import path as search_path
search_path.insert(0,str(Path(__file__).resolve().parent.parent))
from verify_path_v3 import pdf_with_stream

ROOT=Path(__file__).resolve().parents[2]
EXPORTER=ROOT/'target/release/export_geometry_v3'
HAYRO=ROOT/'target/release/opds-engine-probe'
GEOMETRY=Path('/tmp/opds-page-geometry-probe-v3')
KERNEL=Path('/tmp/opds-buffer-probe-large')

def command(rect,kind,ink):
    x,y,w,h=rect
    rgb=' '.join(str(v//255) for v in ink)
    if kind==1: return f'{rgb} rg\n{x} {y} {w} {h} re\nf\n'.encode()
    if kind==3 and h==1: return f'{rgb} RG\n1 w\n{x} {y+0.5} m\n{x+w} {y+0.5} l\nS\n'.encode()
    if kind==3 and w==1: return f'{rgb} RG\n1 w\n{x+0.5} {y} m\n{x+0.5} {y+h} l\nS\n'.encode()
    raise ValueError('unsupported test record')

def main():
    cases=[('normal',192,256,(0,0,192,256),0),('rot90',192,256,(0,0,192,256),90),
           ('rot180',192,256,(0,0,192,256),180),('rot270',192,256,(0,0,192,256),270),
           ('crop90',220,280,(20,30,170,230),90),('a1-90',1684,2384,(0,0,1684,2384),90),
           ('nested-90',192,256,(0,0,192,256),90),('translated-a1',1684,2384,(0,0,1684,2384),90),
           ('restore90',192,256,(0,0,192,256),90),
           ('clip90',192,256,(0,0,192,256),90),('clip-restore90',192,256,(0,0,192,256),90),
           ('clip-intersection90',192,256,(0,0,192,256),90),
           ('clip-a1',1684,2384,(0,0,1684,2384),90)]
    report={'cases':{},'unsupported_pdfs_rejected':0,'invalid_records_rejected':0}
    with tempfile.TemporaryDirectory(prefix='opds-opgm-v3-') as d:
        root=Path(d)
        for name,mw,mh,crop,turn in cases:
            width,height=display_size(crop,turn)
            if name in ('a1-90','translated-a1','clip-a1'):
                records=[(1,(100,80,400,280),COLORS[0]),(3,(120,220,490,221),COLORS[1]),
                         (3,(380,100,381,330),COLORS[2])]
            else:
                records=[(1,(25,20,95,60),COLORS[0]),(3,(30,45,110,46),COLORS[1]),
                         (3,(60,30,61,100),COLORS[2])]
            shift=(7,9) if name in ('nested-90','translated-a1') else (0,0)
            content=b''.join(command(tuple(v-shift[j%2] if j<2 else v for j,v in enumerate(pdf_rect(crop,turn,rect))),kind,ink)
                             for kind,rect,ink in records)
            if name=='nested-90':content=b'q\n1 0 0 1 3 4 cm\nq\n1 0 0 1 4 5 cm\n'+content+b'Q\nQ\n'
            elif name=='translated-a1':content=b'q\n1 0 0 1 7 9 cm\n'+content+b'Q\n'
            elif name=='restore90':
                first=records[0];x,y,w,h=pdf_rect(crop,turn,first[1]);content=(
                    b'q\n1 0 0 1 7 9 cm\n'+command((x-7,y-9,w,h),first[0],first[2])+b'Q\n'
                    +b''.join(command(pdf_rect(crop,turn,rect),kind,ink) for kind,rect,ink in records[1:]))
            expected_records=records
            if name in ('clip90','clip-restore90','clip-intersection90','clip-a1'):
                clip=(180,150,450,350) if name=='clip-a1' else (40,30,100,80)
                cx,cy,cw,ch=pdf_rect(crop,turn,clip)
                clipping=f'{cx} {cy} {cw} {ch} re\nW\nn\n'.encode()
                if name=='clip90':content=clipping+content
                elif name=='clip-a1':content=clipping+content
                elif name=='clip-intersection90':
                    second=(50,35,90,70);sx,sy,sw,sh=pdf_rect(crop,turn,second)
                    content=clipping+f'{sx} {sy} {sw} {sh} re\nW\nn\n'.encode()+content
                    clip=second
                else:
                    first=records[0]
                    content=(b'q\n'+clipping+command(pdf_rect(crop,turn,first[1]),first[0],first[2])+b'Q\n'
                             +b''.join(command(pdf_rect(crop,turn,rect),kind,ink) for kind,rect,ink in records[1:]))
                expected_records=[]
                for idx,(kind,rect,ink) in enumerate(records):
                    if name=='clip-restore90' and idx>0:expected_records.append((kind,rect,ink));continue
                    l,t,r,b=rect;cl,ct,cr,cb=clip
                    result=(max(l,cl),max(t,ct),min(r,cr),min(b,cb))
                    if result[0]<result[2] and result[1]<result[3]:expected_records.append((kind,result,ink))
            pdf=root/f'{name}.pdf';opgm=root/f'{name}.opgm';opdl=root/f'{name}.opdl'
            pdf.write_bytes(pdf_with_stream(content,mw,mh,f'/Rotate {turn} /CropBox [{" ".join(map(str,crop))}]'.encode()))
            run_checked([EXPORTER,pdf,opgm]);run_checked([GEOMETRY,opgm,opdl])
            expected_geom=b'OPGM'+struct.pack('<9I',3,mw,mh,*crop,turn,len(expected_records))+b''.join(
                struct.pack('<5I4B',kind,*rect,*ink,255) for kind,rect,ink in expected_records)
            if opgm.read_bytes()!=expected_geom:raise RuntimeError(name+' geometry differs')
            out=root/f'{name}.rgba';reference=root/f'{name}-hayro.rgba'
            run_checked([KERNEL,'2',opdl,out,'--experimental-large']);run_checked([HAYRO,pdf,'1',reference])
            pix=bytearray(b'\xff\xff\xff\xff'*(width*height))
            for _,rect,ink in expected_records:
                l,t,r,b=rect
                for y in range(t,b):
                    for x in range(l,r):pix[4*(y*width+x):4*(y*width+x)+4]=bytes(ink)+b'\xff'
            if out.read_bytes()!=reference.read_bytes() or out.read_bytes()!=pix:raise RuntimeError(name+' pixels differ')
            report['cases'][name]={'pixels':[width,height],'rgba_bytes_exact':len(pix)}
        for name,mw,mh,crop,turn in [
            ('rgb-normal',192,256,(0,0,192,256),0),
            ('rgb-rot90',192,256,(0,0,192,256),90),
            ('rgb-crop270',220,280,(20,30,170,230),270),
            ('rgb-a1-90',1684,2384,(0,0,1684,2384),90),
        ]:
            width,height=display_size(crop,turn)
            fill=(25,20,95,60) if name!='rgb-a1-90' else (100,80,400,280)
            line=(30,45,110,46) if name!='rgb-a1-90' else (120,220,490,221)
            fx,fy,fw,fh=pdf_rect(crop,turn,fill)
            lx,ly,lw,lh=pdf_rect(crop,turn,line)
            if lh==1:
                stroke=f'{lx} {ly+0.5} m\n{lx+lw} {ly+0.5} l\n'
            else:
                stroke=f'{lx+0.5} {ly} m\n{lx+0.5} {ly+lh} l\n'
            content=(f'0.5 0.25 0.75 rg\n{fx} {fy} {fw} {fh} re\nf\n'
                     f'0.2 0.6 0.8 RG\n1 w\n{stroke}S\n').encode()
            pdf=root/f'{name}.pdf';opgm=root/f'{name}.opgm';opdl=root/f'{name}.opdl'
            pdf.write_bytes(pdf_with_stream(content,mw,mh,f'/Rotate {turn} /CropBox [{" ".join(map(str,crop))}]'.encode()))
            run_checked([EXPORTER,pdf,opgm]);run_checked([GEOMETRY,opgm,opdl])
            expected_records=[(1,fill,(128,64,191)),(3,line,(51,153,204))]
            expected_geom=b'OPGM'+struct.pack('<9I',3,mw,mh,*crop,turn,2)+b''.join(
                struct.pack('<5I4B',kind,*rect,*ink,255) for kind,rect,ink in expected_records)
            if opgm.read_bytes()!=expected_geom:raise RuntimeError(name+' RGB geometry differs')
            out=root/f'{name}.rgba';reference=root/f'{name}-hayro.rgba'
            run_checked([KERNEL,'2',opdl,out,'--experimental-large']);run_checked([HAYRO,pdf,'1',reference])
            pix=bytearray(b'\xff\xff\xff\xff'*(width*height))
            for _,rect,ink in expected_records:
                l,t,r,b=rect
                for y in range(t,b):
                    for x in range(l,r):pix[4*(y*width+x):4*(y*width+x)+4]=bytes(ink)+b'\xff'
            if out.read_bytes()!=reference.read_bytes() or out.read_bytes()!=pix:raise RuntimeError(name+' RGB pixels differ')
            report['cases'][name]={'pixels':[width,height],'rgba_bytes_exact':len(pix)}
        bad=[b'1 0 0 RG\n2 w\n10 20.5 m\n50 20.5 l\nS\n',
             b'1 0 0 RG\n1 w\n10 20 m\n50 20 l\nS\n',
             b'1 0 0 RG\n1 w\n10 20.5 m\n50 20.5 l\n',
             b'1 0 0 RG\n1 w\n10 20.5 m\n50 20.5 l\nS\nq\n',
             b'Q\n1 0 0 rg\n20 20 30 30 re\nf\n',
             b'q\n2 0 0 1 5 5 cm\n1 0 0 rg\n20 20 30 30 re\nf\nQ\n',
             b'q\n1 0 0 1 0.5 5 cm\n1 0 0 rg\n20 20 30 30 re\nf\nQ\n',
             b'q\nq\nq\n1 0 0 rg\n20 20 30 30 re\nf\nQ\nQ\nQ\n',
             b'10 10 30 30 re\nW*\nn\n1 0 0 rg\n20 20 30 30 re\nf\n',
             b'10 10 30 30 re\nW\n1 0 0 rg\n20 20 30 30 re\nf\n',
             b'10 10 30 30 re\nW\nn\n10 10 20 20 re\nW\n',
             b'-0.1 0 0 rg\n20 20 30 30 re\nf\n',
             b'0 1.1 0 RG\n1 w\n10 20.5 m\n50 20.5 l\nS\n']
        for index,stream in enumerate(bad):
            pdf=root/f'bad{index}.pdf';out=root/f'bad{index}.opgm'
            pdf.write_bytes(pdf_with_stream(stream,192,256,b'/Rotate 90'))
            if subprocess.run([EXPORTER,pdf,out],capture_output=True).returncode==0 or out.exists():raise RuntimeError('accepted bad PDF')
        report['unsupported_pdfs_rejected']=len(bad)
        good=bytearray(expected_geom)
        for index,mutated in enumerate((good[:-1],good+b'\x00',good[:40]+struct.pack('<I',5)+good[44:],good[:40]+struct.pack('<I',3)+struct.pack('<4I',0,0,2,2)+bytes([255,0,0,128])+good[64:])):
            source=root/f'bad{index}.opgm';out=root/f'bad{index}.opdl';source.write_bytes(mutated)
            if subprocess.run([GEOMETRY,source,out],capture_output=True).returncode==0 or out.exists():raise RuntimeError('accepted bad OPGM')
        report['invalid_records_rejected']=4
    print(json.dumps(report,indent=2))

if __name__=='__main__':main()
