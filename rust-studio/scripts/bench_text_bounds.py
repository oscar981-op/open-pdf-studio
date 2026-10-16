#!/usr/bin/env python3
"""Alternate preserved baseline/native candidate/Hayro on identical text replays."""
import argparse, hashlib, json, statistics, subprocess, tempfile
from pathlib import Path

def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--baseline',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--pairs',type=int,default=15)
    p.add_argument('--page',type=int,default=6)
    a=p.parse_args(); assert 1<=a.pairs<=30 and 1<=a.page<=49
    dirs={'before':a.baseline.resolve(),'after':Path('target/release').resolve(),'hayro':Path('target/release').resolve()}
    source=next(x for x in json.loads(Path('docs/public-pdf-corpus-2026-10-08.json').read_text()) if x['id']=='albany-contract')
    assert sha(Path(source['path']))==source['sha256']
    hashes={k:{n:sha(d/n) for n in ['jpeg_zoom_probe','opds-text-worker']} for k,d in dirs.items()}
    report={'source_sha256':source['sha256'],'page':a.page,'binary_sha256':hashes,'protocol':'Alternating process triplets per scale. Warm OS cache; initial render includes worker/preparation. Two warm replays per process reuse the prepared scene for both DynLex versions. Hayro creates fresh RenderCache. Byte check every render outside timer. GNU time RSS includes helper. No cold OS cache, equal internal caches or GUI claim.','cases':[]}
    with tempfile.TemporaryDirectory(prefix='opds-text-bounds-bench-') as t:
        folder=Path(t)
        for scale in [.25,1,2]:
            subprocess.run([str(dirs['after']/'backend_probe'),source['path'],str(folder/'0.rgba'),'--hayro',f'--scale={scale}',f'--page={a.page}'],capture_output=True,check=True)
            case={'scale':scale,'pairs':[]};report['cases'].append(case)
            for i in range(a.pairs):
                pair={}
                for name in (['before','after','hayro'] if i%2==0 else ['hayro','after','before']):
                    r=subprocess.run(['/usr/bin/time','-f','%M','-o',str(folder/'rss'),str(dirs[name]/'jpeg_zoom_probe'),source['path'],'hayro' if name=='hayro' else 'dynlex','2',str(folder),str(scale),str(a.page)],capture_output=True,text=True,check=True,timeout=45)
                    v=json.loads(r.stdout);v['peak_rss_kib']=int((folder/'rss').read_text());v['cache_diagnostic']=r.stderr.strip()
                    if name!='hayro': assert 'text_cache=Some((1,' in r.stderr
                    pair[name]=v
                case['pairs'].append(pair)
            def warm(v): return statistics.median(s['ms'] for s in v['samples'] if s['round']>0)
            case['summary']={name:{'initial_ms':statistics.median(v[name]['samples'][0]['ms'] for v in case['pairs']),'warm_ms':statistics.median(warm(v[name]) for v in case['pairs']),'rss_kib':statistics.median(v[name]['peak_rss_kib'] for v in case['pairs'])} for name in dirs}
            case['after_wins']={name:sum(warm(v['after'])<warm(v[name]) for v in case['pairs']) for name in ['before','hayro']}
            a.output.write_text(json.dumps(report,indent=2)+'\n');print(scale,case['summary'],case['after_wins'],flush=True)
    assert hashes=={k:{n:sha(d/n) for n in ['jpeg_zoom_probe','opds-text-worker']} for k,d in dirs.items()}
if __name__=='__main__': main()
