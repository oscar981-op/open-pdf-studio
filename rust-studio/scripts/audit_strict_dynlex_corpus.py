#!/usr/bin/env python3
"""Audit complete real documents/pages/zooms without counting fallback as support."""
import argparse,hashlib,json,os,re,subprocess,tempfile
from pathlib import Path


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--probe',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('pdfs',type=Path,nargs='+');a=p.parse_args()
    report={'protocol':'all pages, scales 0.25/1/2, strict DynLex; unsupported has no speed score; successful output requires full Hayro equality','documents':[],'supported':0,'unsupported':0,'errors':0}
    probe=a.probe.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='opds-strict-corpus-') as temp:
        d=Path(temp)
        for source in a.pdfs:
            pdf=source.resolve(strict=True)
            info=subprocess.run(['pdfinfo',str(pdf)],env=dict(os.environ,LC_ALL='C'),text=True,capture_output=True,check=True).stdout
            pages=int(re.search(r'^Pages:\s+(\d+)',info,re.M)[1])
            if pages>200:raise RuntimeError('audit page bound exceeded')
            doc={'pdf':str(source),'pages':pages,'bytes':pdf.stat().st_size,'sha256':hashlib.sha256(pdf.read_bytes()).hexdigest(),'results':[]};report['documents'].append(doc)
            for page in range(1,pages+1):
                for scale in [.25,1,2]:
                    out=d/'actual.rgba';out.unlink(missing_ok=True)
                    args=[str(probe),str(pdf),str(out),'--dynlex-strict',f'--scale={scale}',f'--page={page}']
                    result=subprocess.run(args,text=True,capture_output=True)
                    row={'page':page,'scale':scale};doc['results'].append(row)
                    if result.returncode:
                        unsupported='No fallback rendered' in result.stderr and not out.exists()
                        row['status']='unsupported' if unsupported else 'error';row['reason']=result.stderr.strip();report['unsupported' if unsupported else 'errors']+=1
                    else:
                        ref=d/'hayro.rgba';args[2]=str(ref);args[3]='--hayro'
                        subprocess.run(args,text=True,capture_output=True,check=True)
                        if out.read_bytes()!=ref.read_bytes():raise RuntimeError(f'{pdf.name} page {page} scale {scale}: mismatch')
                        row['status']='supported_exact';row['rgba_bytes']=out.stat().st_size;report['supported']+=1
            print(pdf.name, pages, {s:sum(r['status']==s for r in doc['results']) for s in ['supported_exact','unsupported','error']},flush=True)
    a.output.write_text(json.dumps(report,indent=2)+'\n')

if __name__=='__main__':main()
