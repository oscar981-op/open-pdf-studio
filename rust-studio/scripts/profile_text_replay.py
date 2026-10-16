import json,subprocess,hashlib,sys,statistics
from pathlib import Path
source=next(x for x in json.loads(Path('docs/public-pdf-corpus-2026-10-08.json').read_text()) if x['id']=='albany-contract')
probe=Path('target/release/text_profile_probe');report={'source_sha256':source['sha256'],'probe_sha256':hashlib.sha256(probe.read_bytes()).hexdigest(),'stages':['raster','bounds_and_command','compose','paper'],'cases':[]}
for scale in [.25,1,2]:
 ref=f'/tmp/opds-text-profile-{scale}.rgba'
 subprocess.run(['target/release/backend_probe',source['path'],ref,'--hayro',f'--scale={scale}','--page=6'],capture_output=True,check=True)
 result=subprocess.run([str(probe),source['path'],'6',str(scale),ref],capture_output=True,text=True,check=True)
 row=json.loads(result.stdout);row['scale']=scale;row['median_stages_ms']=[statistics.median(s['stages_ms'][i] for s in row['samples']) for i in range(4)];report['cases'].append(row)
 print(scale,row['prepare_ms'],row['median_stages_ms'],flush=True)
Path(sys.argv[1]).write_text(json.dumps(report,indent=2)+'\n')
