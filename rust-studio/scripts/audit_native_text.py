#!/usr/bin/env python3
"""Full-page strict native DynLex text audit, with no renderer fallback."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from audit_text_winding import text_fixture
from audit_prepared_text import compare


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(cmd):
    return subprocess.run(list(map(str, cmd)), capture_output=True, text=True, timeout=40)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    backend=Path('target/release/backend_probe').resolve(strict=True)
    worker=backend.with_name('opds-text-worker')
    source=next(x for x in json.loads(Path('docs/public-pdf-corpus-2026-10-08.json').read_text()) if x['id']=='albany-contract')
    pdf=Path(source['path']); assert sha(pdf)==source['sha256']
    font=Path('assets/fonts/LiberationSans-Regular.ttf')
    hashes={'backend':sha(backend),'worker':sha(worker)}
    report={'native_dispatch':True,'source_sha256':sha(pdf),'binary_sha256':hashes,'font_sha256':sha(font),'cases':[],'rejections':[]}
    with tempfile.TemporaryDirectory(prefix='opds-native-text-') as tmp:
        root=Path(tmp)
        cases=[(f'albany-{s}',pdf,6,s) for s in [.25,.5,.87,1,1.15,1.5625,2,3]]
        for rotation in [0,90,180,270]:
            for i,crop in enumerate(['0 0 640 800','8 8 632 792']):
                p=root/f'fixture-{rotation}-{i}.pdf';p.write_bytes(text_fixture(font.read_bytes(),rotation,crop))
                cases.append((p.stem,p,1,1))
        for name,path,page,scale in cases:
            actual,expected=root/f'{name}.rgba',root/f'{name}.hayro'
            result=run([backend,path,actual,'--dynlex-strict',f'--scale={scale}',f'--page={page}'])
            row={'case':name,'source_sha256':sha(path),'page':page,'scale':scale,'exit_code':result.returncode,'diagnostic':result.stderr.strip()}
            if result.returncode==0:
                assert 'PDF=Dynlex' in result.stderr
                ref=run([backend,path,expected,'--hayro',f'--scale={scale}',f'--page={page}'])
                if ref.returncode: raise RuntimeError(ref.stderr)
                row['pixels']=compare(actual,expected)
            elif actual.exists(): raise RuntimeError('partial output after failure')
            report['cases'].append(row);print(name,row.get('pixels',row['diagnostic']),flush=True)
        # Same binary with a different application directory: never modify the
        # live helper or running user app to simulate packaging/process failures.
        isolated=root/'isolated';isolated.mkdir();probe=isolated/'backend_probe'
        try: os.link(backend,probe)
        except OSError: shutil.copy2(backend,probe)
        helper=isolated/'opds-text-worker'
        for name,body in [('missing',None),('crash','kill -ABRT $$'),('bad-ipc','cat >/dev/null; printf PWPG')]:
            if body is not None:
                helper.write_text('#!/bin/sh\n'+body+'\n');helper.chmod(0o700)
            output=root/f'{name}.rgba'
            r=run([probe,pdf,output,'--dynlex-strict','--scale=1','--page=6'])
            report['rejections'].append({'case':name,'rejected_without_bitmap':r.returncode!=0 and not output.exists() and 'No fallback rendered' in r.stderr,'diagnostic':r.stderr.strip()})
            helper.unlink(missing_ok=True)
    assert hashes=={'backend':sha(backend),'worker':sha(worker)},'binary changed'
    report['all_exact']=all(x.get('pixels',{}).get('exact') for x in report['cases'])
    report['all_rejected']=all(x['rejected_without_bitmap'] for x in report['rejections'])
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    if not report['all_exact'] or not report['all_rejected']: raise SystemExit('native text audit failed')


if __name__=='__main__': main()
