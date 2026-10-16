#!/usr/bin/env python3
"""Re-audit the fixed 30 public cases through strict native DynLex dispatch."""
import argparse,hashlib,json,os,tempfile
from pathlib import Path
from bench_public_corpus import PAGES,run,compare

def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    probe=Path('target/release/backend_probe').resolve(strict=True);worker=probe.with_name('opds-text-worker')
    hashes={'probe':sha(probe),'worker':sha(worker)}
    report={'binary_sha256':hashes,'protocol':'Fixed ten public pages at 25/100/200%; strict native DynLex, independent full Hayro comparison for accepted pages; single runs are not performance benchmarks.','cases':[],'supported':0,'unsupported':0,'errors':0,'wrong':0}
    with tempfile.TemporaryDirectory(prefix='opds-native-text-public-') as tmp:
        for source in json.loads(Path('docs/public-pdf-corpus-2026-10-08.json').read_text()):
            if source['status']!='downloaded':continue
            pdf=Path(source['path']);assert sha(pdf)==source['sha256']
            for page in PAGES[source['id']]:
                for scale in [.25,1,2]:
                    row,data=run(probe,pdf,'dynlex-strict',page,scale,Path(tmp),dict(os.environ))
                    item={'id':source['id'],'source_sha256':source['sha256'],'page':page,'scale':scale,'dynlex':row};report['cases'].append(item)
                    if data is not None:
                        ref,expected=run(probe,pdf,'hayro',page,scale,Path(tmp),dict(os.environ))
                        item['pixels']=compare(row,data,ref,expected)
                        report['supported' if item['pixels'].get('different_rgba_channels')==0 else 'wrong']+=1
                    else:report['unsupported' if row['status']=='unsupported' else 'errors']+=1
                    print(source['id'],page,scale,row['status'],flush=True)
                    a.output.write_text(json.dumps(report,indent=2)+'\n')
    assert hashes=={'probe':sha(probe),'worker':sha(worker)}
    print({k:report[k] for k in ['supported','unsupported','errors','wrong']})
    if report['wrong'] or report['errors']:raise SystemExit('audit failed')
if __name__=='__main__':main()
