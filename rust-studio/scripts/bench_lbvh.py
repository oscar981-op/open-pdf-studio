#!/usr/bin/env python3
"""Warm paired viewport passes against a preserved grid-baseline binary.
Generated PDFs are controlled capacity fixtures, not production drawings.
Every probe compares every tile byte to Hayro outside its render timer.
"""
import argparse, hashlib, json, statistics, subprocess, tempfile
from pathlib import Path


def fixture(path, layout):
    ops=[]
    for i in range(6000):
        rgb=('1 0 0','0 0 1','0 1 0')[i%3]
        x,y,w,h = (4+(i%100)*8,4+(i//100)*12,4,4) if layout=='sparse' else (100,100,24,24)
        ops.extend((f'{rgb} rg',f'{x} {y} {w} {h} re','f'))
    content=('\n'.join(ops)+'\n').encode()
    objects=[b'<< /Type /Catalog /Pages 2 0 R >>',b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1024 768] /Resources << >> /Contents 4 0 R >>',f'<< /Length {len(content)} >>\nstream\n'.encode()+content+b'endstream']
    data=bytearray(b'%PDF-1.7\n'); offsets=[0]
    for i,obj in enumerate(objects,1):
        offsets.append(len(data));data+=f'{i} 0 obj\n'.encode()+obj+b'\nendobj\n'
    xref=len(data);data+=f'xref\n0 {len(objects)+1}\n0000000000 65535 f \n'.encode()
    for offset in offsets[1:]: data+=f'{offset:010d} 00000 n \n'.encode()
    data+=f'trailer\n<< /Size {len(objects)+1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode();path.write_bytes(data)


def run(binary,pdf,tile,directory):
    rss=directory/'rss.txt'
    result=subprocess.run(['/usr/bin/time','-f','%M','-o',str(rss),str(binary),str(pdf),'1',str(tile)],text=True,capture_output=True,check=True)
    report=json.loads(result.stdout);report['whole_process_peak_rss_kib']=int(rss.read_text());return report


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--baseline',type=Path,required=True);p.add_argument('--candidate',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    binaries={'grid':a.baseline.resolve(),'lbvh':a.candidate.resolve()}
    report={'protocol':'3 alternating process pairs; 15 warm full-page tile passes per process; OS cache not cleared; timing excludes Hayro/pixel comparison; RSS includes oracle and parsing','binaries':{k:{'path':str(v),'sha256':hashlib.sha256(v.read_bytes()).hexdigest()} for k,v in binaries.items()},'cases':[]}
    with tempfile.TemporaryDirectory(prefix='opds-lbvh-') as temp:
        d=Path(temp)
        for layout in ['sparse','overlap']:
            pdf=d/f'{layout}.pdf';fixture(pdf,layout)
            case={'layout':layout,'commands':6000,'pdf_sha256':hashlib.sha256(pdf.read_bytes()).hexdigest(),'tile':128,'pairs':[]};report['cases'].append(case)
            for i in range(3):
                pair={}
                for name in (['grid','lbvh'] if i%2==0 else ['lbvh','grid']): pair[name]=run(binaries[name],pdf,128,d)
                case['pairs'].append(pair)
            case['median_pass_ms']={k:statistics.median(t for pair in case['pairs'] for t in pair[k]['pass_ms']) for k in binaries}
            print(layout,case['median_pass_ms'],flush=True)
    a.output.write_text(json.dumps(report,indent=2)+'\n')

if __name__=='__main__': main()
