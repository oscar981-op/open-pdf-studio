#!/usr/bin/env python3
"""Strict four-renderer CLI benchmark. No rejected page is awarded a timing score."""
import argparse,hashlib,json,os,platform,re,signal,statistics,subprocess,tempfile,time
from pathlib import Path
from PIL import Image,ImageChops

BACKENDS={'hayro':'Hayro','pdfium-strict':'Pdfium','dynlex-strict':'Dynlex','own-rust-strict':'OwnRust'}
PAGES={'attention':[2,3],'albany-contract':[1,6],'albany-drawings':[3],
       'nps-schematic':[3,71],'tfl-tube':[1],'nps-drawing-manual':[48],'usgs-topo-guide':[7]}

def run(probe,pdf,backend,page,scale,folder,env):
    out=folder/(backend+'.rgba');out.unlink(missing_ok=True)
    rss=folder/(backend+'.rss');rss.unlink(missing_ok=True)
    args=['/usr/bin/time','-f','%M','-o',str(rss),str(probe),str(pdf),str(out),'--'+backend,f'--scale={scale}',f'--page={page}']
    start=time.perf_counter_ns()
    process=subprocess.Popen(args,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,env=env,start_new_session=True)
    try:stdout,stderr=process.communicate(timeout=120)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid,signal.SIGKILL);process.communicate()
        return {'status':'timeout','reason':'120-second process-group limit'},None
    wall=(time.perf_counter_ns()-start)/1e6
    if process.returncode:
        if out.exists():raise RuntimeError('failure wrote a misleading output')
        # GNU time propagates signal exits as 128+signal; never count a crash as unsupported.
        status='unsupported' if process.returncode == 1 and ('No fallback rendered' in stderr or backend=='own-rust-strict' or 'limit' in stderr or 'TooLarge' in stderr) else 'error'
        return {'status':status,'reason':stderr.strip(),'returncode':process.returncode},None
    match=re.search(r'(\d+)x(\d+) (?:PDF=)?(Hayro|Pdfium|Dynlex|OwnRust)',stderr)
    if not match or match[3]!=BACKENDS[backend]:raise RuntimeError('unexpected or fallback renderer: '+stderr)
    size=[int(match[1]),int(match[2])];data=out.read_bytes()
    if len(data)!=size[0]*size[1]*4:raise RuntimeError('invalid bitmap size')
    row={'status':'rendered','used':match[3],'size':size,'wall_ms':round(wall,3),
         'peak_rss_kib':int(rss.read_text().strip().splitlines()[-1]),'sha256':hashlib.sha256(data).hexdigest()}
    return row,data

def compare(row,data,ref,reference):
    if row['size']!=ref['size']:return {'comparable':False,'reason':'different bitmap dimensions','reference_size':ref['size']}
    diff=ImageChops.difference(Image.frombytes('RGBA',tuple(row['size']),data),Image.frombytes('RGBA',tuple(ref['size']),reference))
    hist=diff.histogram();channels=sum(sum(hist[c*256+1:(c+1)*256]) for c in range(4))
    masks=diff.split();mask=masks[0]
    for m in masks[1:]:mask=ImageChops.lighter(mask,m)
    pixels=sum(mask.histogram()[1:])
    return {'comparable':True,'different_pixels':pixels,'different_rgba_channels':channels,
            'mean_absolute_channel_error':sum(i*hist[c*256+i] for c in range(4) for i in range(256))/len(data)}

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--manifest',type=Path,required=True);p.add_argument('--probe',type=Path,required=True)
    p.add_argument('--pdfium-library',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--rounds',type=int,default=7);a=p.parse_args()
    if not 1<=a.rounds<=30:p.error('rounds 1..30')
    probe=a.probe.resolve(strict=True);library=a.pdfium_library.resolve(strict=True)
    env=dict(os.environ,OPDS_PDFIUM_LIBRARY=str(library));manifest=json.loads(a.manifest.read_text())
    report={'protocol':'Strict engines only; same PDF/page/requested scale; untimed preflight then rotated sequential backend order; new process each sample; warm OS cache; includes read/parse/render/RGBA write and process/time-wrapper startup. No GUI or cached zoom, no cold-cache claim. Unsupported has no speed score. Dimension mismatches are not compared as equal-sized pixels.',
            'rounds':a.rounds,'probe_sha256':hashlib.sha256(probe.read_bytes()).hexdigest(),
            'pdfium_sha256':hashlib.sha256(library.read_bytes()).hexdigest(),'platform':platform.platform(),
            'cpu':next((l.split(':',1)[1].strip() for l in Path('/proc/cpuinfo').read_text().splitlines() if l.startswith('model name')),'unknown'),
            'cases':[]}
    a.output.parent.mkdir(parents=True,exist_ok=True)
    def save():a.output.write_text(json.dumps(report,indent=2)+'\n')
    with tempfile.TemporaryDirectory(prefix='opds-public-four-') as temp:
        folder=Path(temp)
        for source in manifest:
            if source['status']!='downloaded':continue
            pdf=Path(source['path']).resolve(strict=True)
            if hashlib.sha256(pdf.read_bytes()).hexdigest()!=source['sha256']:raise RuntimeError('source hash changed')
            for page in PAGES[source['id']]:
                for scale in [.25,1,2]:
                    case={'id':source['id'],'source_sha256':source['sha256'],'page':page,'scale':scale,'backends':{}}
                    report['cases'].append(case);pre={}
                    for b in BACKENDS:
                        row,data=run(probe,pdf,b,page,scale,folder,env);pre[b]=(row,data)
                        case['backends'][b]={'preflight':row,'samples':[]}
                    ref,refdata=pre['hayro']
                    for b,(row,data) in pre.items():
                        if data is not None and refdata is not None:case['backends'][b]['pixels_vs_hayro']=compare(row,data,ref,refdata)
                    del refdata
                    supported=[b for b in BACKENDS if pre[b][1] is not None]
                    for n in range(a.rounds):
                        order=supported[n%len(supported):]+supported[:n%len(supported)] if supported else []
                        for b in order:
                            row,data=run(probe,pdf,b,page,scale,folder,env)
                            if row['status']!='rendered' or row['sha256']!=pre[b][0]['sha256']:raise RuntimeError('inconsistent repeated output: '+str(case))
                            row['round']=n+1;case['backends'][b]['samples'].append(row)
                    for b in supported:
                        samples=case['backends'][b]['samples'];case['backends'][b]['summary']={
                            'median_wall_ms':statistics.median(x['wall_ms'] for x in samples),
                            'min_wall_ms':min(x['wall_ms'] for x in samples),'max_wall_ms':max(x['wall_ms'] for x in samples),
                            'median_peak_rss_kib':statistics.median(x['peak_rss_kib'] for x in samples)}
                    if 'hayro' in supported and 'pdfium-strict' in supported:
                        case['pdfium_wins_pairs']=sum(p['wall_ms']<h['wall_ms'] for p,h in zip(case['backends']['pdfium-strict']['samples'],case['backends']['hayro']['samples']))
                    save();print(source['id'],page,scale,{b:case['backends'][b].get('summary',{}).get('median_wall_ms',case['backends'][b]['preflight']['status']) for b in BACKENDS},flush=True)
    save()

if __name__=='__main__':main()
