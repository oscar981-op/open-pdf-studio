#!/usr/bin/env python3
"""Paired strict native text replays, with byte checks and explicit cache scope."""
import argparse,hashlib,json,statistics,subprocess,tempfile
from pathlib import Path

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);p.add_argument('--pairs',type=int,default=15);p.add_argument('--page',type=int,default=6);a=p.parse_args()
    if not 1<=a.pairs<=30:p.error('pairs 1..30')
    if not 1<=a.page<=49:p.error('Albany page 1..49')
    root=Path('target/release').resolve();probe=root/'jpeg_zoom_probe';worker=root/'opds-text-worker';renderer=root/'backend_probe'
    hashes={n:sha(root/n) for n in ['jpeg_zoom_probe','opds-text-worker','backend_probe']}
    source=next(x for x in json.loads(Path('docs/public-pdf-corpus-2026-10-08.json').read_text()) if x['id']=='albany-contract');pdf=Path(source['path']);assert sha(pdf)==source['sha256']
    report={'source_sha256':sha(pdf),'page':a.page,'binary_sha256':hashes,'protocol':'Alternating paired new processes, warm OS cache. Initial render includes native preparation/worker IPC; two warm replays use the same page/scale scene. Each fresh RGBA buffer is byte checked outside timer. Current Hayro creates RenderCache per render; DynLex reuses geometry. No equalized internal caches, kernel-only, cold OS cache or GUI claim. GNU time peak RSS includes preparation subprocess and verification.','cases':[]}
    with tempfile.TemporaryDirectory(prefix='opds-native-text-bench-') as tmp:
        folder=Path(tmp)
        for scale in [.25,1,2]:
            subprocess.run([str(renderer),str(pdf),str(folder/'0.rgba'),'--hayro',f'--scale={scale}',f'--page={a.page}'],capture_output=True,check=True)
            case={'scale':scale,'pairs':[]};report['cases'].append(case)
            for i in range(a.pairs):
                pair={}
                for mode in (['dynlex','hayro'] if i%2==0 else ['hayro','dynlex']):
                    r=subprocess.run(['/usr/bin/time','-f','%M','-o',str(folder/'rss'),str(probe),str(pdf),mode,'2',str(folder),str(scale),str(a.page)],capture_output=True,text=True,check=True,timeout=45)
                    sample=json.loads(r.stdout);sample['peak_rss_kib']=int((folder/'rss').read_text());sample['cache_diagnostic']=r.stderr.strip()
                    if mode=='dynlex' and 'text_cache=Some((1,' not in r.stderr:raise RuntimeError('preparation was not reused')
                    pair[mode]=sample
                case['pairs'].append(pair)
            def warm(sample):return statistics.median(x['ms'] for x in sample['samples'] if x['round']>0)
            case['summary']={mode:{'initial_ms':statistics.median(x[mode]['samples'][0]['ms'] for x in case['pairs']),'warm_ms':statistics.median(warm(x[mode]) for x in case['pairs']),'peak_rss_kib':statistics.median(x[mode]['peak_rss_kib'] for x in case['pairs'])} for mode in ['dynlex','hayro']}
            case['dynlex_wins']=sum(warm(x['dynlex'])<warm(x['hayro']) for x in case['pairs'])
            a.output.write_text(json.dumps(report,indent=2)+'\n');print(scale,case['summary'],case['dynlex_wins'],flush=True)
    assert hashes=={n:sha(root/n) for n in hashes},'binary changed'
if __name__=='__main__':main()
