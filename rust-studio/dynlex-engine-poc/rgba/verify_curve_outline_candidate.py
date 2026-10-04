#!/usr/bin/env python3
"""Measure an independent butt-cap outline candidate against recorded oracles."""
import argparse
import json
import subprocess
import tempfile
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
CONTROLS = {
    'arch': ((20.5,111.5),(35.5,61.5),(75.5,61.5),(90.5,111.5)),
    's-bend': ((20.5,111.5),(55.5,51.5),(55.5,171.5),(90.5,111.5)),
    'shallow': ((20.5,111.5),(43.5,93.5),(67.5,93.5),(90.5,111.5)),
    'asymmetric': ((25.5,125.5),(39.5,48.5),(103.5,79.5),(111.5,133.5)),
}

def geometry(path):
    return [(parts[0], [float(x) for x in parts[1:]]) for line in path.read_text().splitlines()
            if (parts := line.split()) and not line.startswith('#')]

def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('candidate','flattener','scene_tool','rasterizer'):
        p.add_argument('--'+name.replace('_','-'), required=True, type=Path)
    p.add_argument('--json',type=Path)
    args=p.parse_args()
    tools={name:str(getattr(args,name).resolve(strict=True)) for name in ('candidate','flattener','scene_tool','rasterizer')}
    report={'scope':'diagnostic one-cubic 1px butt-cap; never used by app or PDF adapter', 'fixtures':{}}
    with tempfile.TemporaryDirectory(prefix='opds-outline-candidate-') as folder:
        root=Path(folder)
        inp,outline,lines,scene,raw=(root/name for name in ('input.txt','outline.txt','flat.txt','scene.opln','coverage.raw'))
        for name,controls in CONTROLS.items():
            inp.write_text('M %s %s\nC %s %s %s %s %s %s\n' % tuple(v for pair in controls for v in pair))
            subprocess.run([tools['candidate'],str(inp),str(outline)],check=True)
            subprocess.run([tools['flattener'],str(outline),str(lines)],check=True,capture_output=True)
            subprocess.run([tools['scene_tool'],'encode',str(lines),str(scene)],check=True,capture_output=True)
            subprocess.run([tools['rasterizer'],str(scene),str(raw)],check=True,capture_output=True)
            reference=geometry(HERE/f'curve-outline-{name}.txt')
            actual=geometry(outline)
            if [op for op,_ in actual]!=[op for op,_ in reference]:
                raise RuntimeError('outline command order changed')
            max_delta=max(abs(a-b) for (_,av),(_,bv) in zip(actual,reference) for a,b in zip(av,bv))
            oracle=zlib.decompress((HERE/f'curve-coverage-{name}.zlib').read_bytes())
            pixels=raw.read_bytes()
            if len(pixels)!=256*192 or len(oracle)!=256*192: raise RuntimeError('bad coverage size')
            mismatches=[(i,x,y) for i,(x,y) in enumerate(zip(pixels,oracle)) if x!=y]
            report['fixtures'][name]={
                'max_outline_coordinate_delta_px':max_delta,
                'coverage_mismatched_pixels':len(mismatches),
                'max_coverage_delta':max((abs(x-y) for _,x,y in mismatches),default=0),
                'first_mismatch':{'x':mismatches[0][0]%256,'y':mismatches[0][0]//256,
                                  'candidate':mismatches[0][1],'oracle':mismatches[0][2]} if mismatches else None,
            }
        for label,content in {
            'missing-curve':'M 0 0\n', 'nan':'M 0 0\nC NaN 1 2 3 4 5\n',
            'extra':'M 0 0\nC 1 2 3 4 5 6\nZ\n',
            'zero':'M 0 0\nC 0 0 0 0 0 0\n',
            'huge':'M 0 0\nC 1 2 3 4 5000 6\n',
        }.items():
            inp.write_text(content);outline.unlink(missing_ok=True)
            result=subprocess.run([tools['candidate'],str(inp),str(outline)],capture_output=True)
            if result.returncode==0 or outline.exists():raise RuntimeError('accepted '+label)
        report['invalid_cases_rejected']=5
    if args.json: args.json.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))

if __name__=='__main__': main()
