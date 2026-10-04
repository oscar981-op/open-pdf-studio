#!/usr/bin/env python3
"""Check bounded scalar cubic flattening geometry and DynLex coverage."""
import argparse
import json
import subprocess
import tempfile
import zlib
from pathlib import Path

HERE=Path(__file__).resolve().parent


def segments(path):
    return [tuple(map(float,line.split())) for line in path.read_text().splitlines()
            if line and not line.startswith('#')]


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--flattener',type=Path,required=True)
    parser.add_argument('--rasterizer',type=Path,required=True)
    parser.add_argument('--scene-tool',type=Path,required=True)
    parser.add_argument('--expected-report',type=Path)
    parser.add_argument('--json',type=Path)
    args=parser.parse_args()
    flat,raster,scene_tool=(getattr(args,key).resolve(strict=True) for key in ('flattener','rasterizer','scene_tool'))
    report={'note':'bounded scalar f32 candidate; Kurbo stroke outlines remain fixture input, no Vello runtime call','fixtures':{}}
    with tempfile.TemporaryDirectory(prefix='opds-scalar-flatten-') as folder:
        work=Path(folder);lines=work/'candidate.txt';scene=work/'candidate.opln';output=work/'coverage.raw'
        for name in ('arch','s-bend','shallow'):
            result=subprocess.run([flat,HERE/f'curve-outline-{name}.txt',lines],check=True,capture_output=True,text=True)
            subprocess.run([scene_tool,'encode',lines,scene],check=True,capture_output=True)
            subprocess.run([scene_tool,'validate',scene],check=True,capture_output=True)
            subprocess.run([raster,scene,output],check=True,capture_output=True)
            oracle=zlib.decompress((HERE/f'curve-coverage-{name}.zlib').read_bytes())
            actual=output.read_bytes()
            if len(actual)!=256*192:raise RuntimeError('wrong coverage length')
            own=segments(lines);ref=segments(HERE/f'curve-flat-{name}.txt')
            if len(own)!=len(ref):raise RuntimeError('segment count differs')
            diffs=[max(abs(x-y) for x,y in zip(a,b)) for a,b in zip(own,ref)]
            mismatches=[i for i,(x,y) in enumerate(zip(actual,oracle)) if x!=y]
            if mismatches:raise RuntimeError(f'{name}: {len(mismatches)} coverage bytes differ')
            report['fixtures'][name]={
                'segments':len(own),'quadratic_subdivisions':result.stderr.strip().split('quadratic subdivisions ')[-1],
                'segment_coordinate_differences_over_1e_6':sum(d>1e-6 for d in diffs),
                'max_segment_coordinate_delta':max(diffs),
                'coverage_bytes_exact':len(actual),
            }
        for name,source in {
            'open':'M 0 0\nC 1 1 2 2 3 3\n',
            'nonfinite':'M 0 0\nC NaN 0 2 2 3 3\nZ\n',
            'unknown':'M 0 0\nQ 1 1 2 2\nZ\n',
            'degenerate':'M 0 0\nC 0 0 0 0 0 0\nL 1 0\nZ\n',
            'oversized':'M 0 0\nL 5000 0\nL 0 1\nZ\n',
            'too-many-curves':'M 0 0\n'+('C 1 0 2 0 3 0\n'*17)+'Z\n',
        }.items():
            bad=work/f'bad-{name}.txt';bad.write_text(source);lines.unlink(missing_ok=True)
            result=subprocess.run([flat,bad,lines],capture_output=True)
            if result.returncode==0 or lines.exists():raise RuntimeError(f'accepted {name}')
        report['invalid_cases_rejected']=6
    if args.expected_report and report!=json.loads(args.expected_report.read_text()):
        raise RuntimeError('scalar flatten report differs')
    if args.json:args.json.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))

if __name__=='__main__':main()
