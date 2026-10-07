#!/usr/bin/env python3
"""Compare actual DynLex-only output with Hayro; unsupported is never a timing win."""
import argparse,json,os,statistics,subprocess,tempfile
from pathlib import Path
from bench_four_backends import run_one


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--probe',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--rounds',type=int,default=15);p.add_argument('--scale',type=float,default=1)
    p.add_argument('pdfs',type=Path,nargs='+');a=p.parse_args()
    if not 1<=a.rounds<=100 or not .01<=a.scale<=8: p.error('invalid bounded rounds/scale')
    probe=a.probe.resolve(strict=True)
    report={'protocol':'alternating paired new processes, warm OS cache, page 1; includes read/open/render/RGBA write; strict DynLex without fallback; unsupported has no render-time score','rounds':a.rounds,'scale':a.scale,'cases':[]}
    with tempfile.TemporaryDirectory(prefix='opds-strict-bench-') as temp:
        d=Path(temp)
        for source in a.pdfs:
            pdf=source.resolve(strict=True);out=d/'preflight.rgba';out.unlink(missing_ok=True)
            pre=subprocess.run([str(probe),str(pdf),str(out),'--dynlex-strict',f'--scale={a.scale}'],text=True,capture_output=True)
            case={'pdf':str(source),'supported':pre.returncode==0};report['cases'].append(case)
            if pre.returncode:
                if out.exists() or 'No fallback rendered' not in pre.stderr: raise RuntimeError(pre.stderr)
                case['reason']=pre.stderr.strip();print(pdf.name,'unsupported (no fallback)',flush=True);continue
            samples=[];case['pairs']=samples
            for i in range(a.rounds):
                pair={}
                for backend in (['dynlex-strict','hayro'] if i%2==0 else ['hayro','dynlex-strict']):
                    pair[backend]=run_one(probe,pdf,backend,a.scale,d,dict(os.environ))
                if pair['dynlex-strict']['used']!='Dynlex':raise RuntimeError('unexpected renderer')
                actual=pair['dynlex-strict'].pop('rgba');reference=pair['hayro'].pop('rgba')
                if actual!=reference:raise RuntimeError(f'{pdf.name}: pixel mismatch')
                samples.append(pair)
            case['all_rgba_equal']=True
            case['medians']={b:{'wall_ms':statistics.median(row[b]['wall_ms'] for row in samples),'peak_rss_kib':statistics.median(row[b]['peak_rss_kib'] for row in samples)} for b in ['dynlex-strict','hayro']}
            case['dynlex_wins']=sum(row['dynlex-strict']['wall_ms']<row['hayro']['wall_ms'] for row in samples)
            print(pdf.name,case['medians'],case['dynlex_wins'],flush=True)
    a.output.write_text(json.dumps(report,indent=2)+'\n')

if __name__=='__main__':main()
