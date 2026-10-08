#!/usr/bin/env python3
"""Paired real-scan app-route zoom replay, with original JPEG cache and byte checks."""
import argparse, hashlib, json, statistics, subprocess, tempfile
from pathlib import Path


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--probe',type=Path,required=True)
    p.add_argument('--render-probe',type=Path,required=True)
    p.add_argument('--manifest',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--pairs',type=int,default=15)
    p.add_argument('--rounds',type=int,default=3)
    a=p.parse_args()
    if not 1<=a.pairs<=30 or not 1<=a.rounds<=30: p.error('bounded pairs/rounds exceeded')
    source=next(x for x in json.loads(a.manifest.read_text()) if x['id']=='albany-contract')
    pdf=Path(source['path']).resolve(strict=True)
    if hashlib.sha256(pdf.read_bytes()).hexdigest()!=source['sha256']: raise ValueError('source hash changed')
    report={'source':source, 'protocol':'Alternating paired new processes, warm OS cache. Each process retains PDF/document for initial .25/1/2 and repeated .25/1/2 renders. Every result byte checked outside render timer. DynLex caches original RGB; current Hayro app route creates RenderCache per render. This measures current app routes, not equal internal caches or a kernel-only comparison. First render has empty engine cache, not cold OS cache. /usr/bin/time measures whole-process RSS including byte verification. No UI latency measurement.', 'rounds':a.rounds,'pairs':[]}
    with tempfile.TemporaryDirectory(prefix='opds-jpeg-zoom-') as temp:
        d=Path(temp)
        for i,scale in enumerate([.25,1,2]):
            subprocess.run([str(a.render_probe.resolve()),str(pdf),str(d/f'{i}.rgba'),'--hayro',f'--scale={scale}'],check=True,capture_output=True,timeout=120)
        for index in range(a.pairs):
            pair={}
            for mode in (['dynlex','hayro'] if index%2==0 else ['hayro','dynlex']):
                r=subprocess.run(['/usr/bin/time','-f','%M','-o',str(d/'rss'),str(a.probe.resolve()),str(pdf),mode,str(a.rounds),str(d)],check=True,capture_output=True,text=True,timeout=180)
                sample=json.loads(r.stdout);sample['peak_rss_kib']=int((d/'rss').read_text())
                if mode=='dynlex' and (sample['decodes']!=1 or sample['retained_rgb_bytes']!=5100*6600*3): raise ValueError('unexpected cache behavior')
                pair[mode]=sample
            report['pairs'].append(pair)
            print(f'pair {index+1}/{a.pairs} verified',flush=True)
    summary={}
    for mode in ['dynlex','hayro']:
        rows=[r[mode] for r in report['pairs']]
        summary[mode]={'first_render_ms':statistics.median(r['samples'][0]['ms'] for r in rows),'peak_rss_kib':statistics.median(r['peak_rss_kib'] for r in rows),'warm_render_ms':{str(s):statistics.median(x['ms'] for r in rows for x in r['samples'] if x['round']>0 and x['scale']==s) for s in [.25,1,2]}}
    report['summary']=summary
    report['paired_warm_dynlex_wins']={str(s):sum(statistics.median(x['ms'] for x in r['dynlex']['samples'] if x['round']>0 and x['scale']==s)<statistics.median(x['ms'] for x in r['hayro']['samples'] if x['round']>0 and x['scale']==s) for r in report['pairs']) for s in [.25,1,2]}
    a.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(summary,indent=2))


if __name__=='__main__':main()
