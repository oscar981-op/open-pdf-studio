#!/usr/bin/env python3
"""Full WinAnsi table and DeviceRGB/Gray text differential audit via native route."""
import argparse, hashlib, json, random, subprocess, tempfile
from pathlib import Path
from audit_text_winding import difference

def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def fixture(font, content, rotation=0, crop='0 0 640 800'):
    stream=lambda d,b:b'<< '+d+b' /Length '+str(len(b)).encode()+b' >>\nstream\n'+b+b'\nendstream'
    objs=[b'<< /Type /Catalog /Pages 2 0 R >>',b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
          f'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 640 800] /CropBox [{crop}] /Rotate {rotation} /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>'.encode(),
          stream(b'',content),b'<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSans /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 255 /Widths ['+b'600 '*224+b'] /FontDescriptor 6 0 R >>',
          b'<< /Type /FontDescriptor /FontName /LiberationSans /Flags 32 /FontBBox [-543 -303 1301 980] /ItalicAngle 0 /Ascent 905 /Descent -212 /CapHeight 687 /StemV 80 /FontFile2 7 0 R >>',stream(b'',font)]
    b=bytearray(b'%PDF-1.7\n');off=[0]
    for i,o in enumerate(objs,1):off.append(len(b));b+=f'{i} 0 obj\n'.encode()+o+b'\nendobj\n'
    x=len(b);b+=f'xref\n0 {len(off)}\n0000000000 65535 f \n'.encode()
    for o in off[1:]:b+=f'{o:010} 00000 n \n'.encode()
    b+=f'trailer << /Root 1 0 R /Size {len(off)} >>\nstartxref\n{x}\n%%EOF\n'.encode()
    return bytes(b)

def content(seed):
    rng=random.Random(seed);out=['/Artifact BMC /Span <<>> BDC 0.17 0.35 0.63 rg 12 12 616 776 re f* EMC']
    # Every WinAnsi byte 32..255, including PDF-specific bullet/space/hyphen aliases.
    # Inputs immediately around half-byte rounding plus arbitrary RGB/Gray.
    for i,code in enumerate(range(32,256)):
        values=[min(1.,max(0.,(rng.randrange(255)+.5)/255+rng.choice([-1e-8,0,1e-8]))) for _ in range(3)]
        color=f'{values[0]:.12f} g' if i%2 else ' '.join(f'{v:.12f}' for v in values)+' rg'
        x=22+(i%14)*43.25;y=748-(i//14)*43.125
        out += ['q',color,f'BT /F 17 Tf 1 0 0 1 {x} {y} Tm <{code:02x}> Tj ET','Q']
    out+=['0.613 g BT /F 12 Tf 1 Tc 3 Tw 1 0 0 1 20.375 40.25 Tm <41a0422043ad44d745> Tj ET',
          'q 0.25 0.51 0.79 rg BT /F 18 Tf 1 0 0 1 190.25 40.25 Tm <c0c4c5c6d7e0e4e5e6f7ff> Tj ET Q',
          'BT /F 12 Tf 1 0 0 1 190.25 42.125 Tm (overlap) Tj ET']
    out.append('EMC')
    return ('\n'.join(out)+'\n').encode()

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    probe=Path('target/release/backend_probe').resolve();worker=probe.with_name('opds-text-worker');font=Path('assets/fonts/LiberationSans-Regular.ttf')
    hashes={'probe':sha(probe),'worker':sha(worker),'font':sha(font)};report={'hashes':hashes,'cases':[],'rejected':[]}
    with tempfile.TemporaryDirectory(prefix='opds-winansi-') as t:
        d=Path(t)
        for seed in [108,731]:
            for rotation in [0,90,180,270]:
                for crop in ['0 0 640 800','8 8 632 792']:
                    pdf=d/'fixture.pdf';pdf.write_bytes(fixture(font.read_bytes(),content(seed),rotation,crop))
                    for scale in [.25,1,2]:
                        out=d/'actual';out.unlink(missing_ok=True)
                        args=[str(probe),str(pdf),str(out),'--dynlex-strict',f'--scale={scale}']
                        r=subprocess.run(args,text=True,capture_output=True,timeout=40)
                        row={'seed':seed,'rotation':rotation,'crop':crop,'scale':scale,'source_sha256':sha(pdf),'exit':r.returncode}
                        if r.returncode:row['diagnostic']=r.stderr;row['exact']=False
                        else:
                            args[2]=str(d/'expected');args[3]='--hayro';subprocess.run(args,capture_output=True,check=True,timeout=40)
                            actual=out.read_bytes();expected=(d/'expected').read_bytes();row['bytes']=len(actual);row['different_channels']=sum(x!=y for x,y in zip(actual,expected));row['exact']=actual==expected
                        report['cases'].append(row);print(seed,rotation,crop,scale,row['exact'],row.get('different_channels'),flush=True)
        for c in [b'BT /F 12 Tf <01> Tj ET',b'1.01 g',b'0 -0.1 1 rg',b'0 0 rg',b'(x) g',b'/OC BMC EMC',b'/Span BMC',b'1 BMC EMC',b'10 10 20 20 re 2 2 4 4 re f*']:
            pdf=d/'bad.pdf';pdf.write_bytes(fixture(font.read_bytes(),c));out=d/'bad.rgba';out.unlink(missing_ok=True)
            r=subprocess.run([str(probe),str(pdf),str(out),'--dynlex-strict'],capture_output=True,timeout=40)
            assert r.returncode and not out.exists();report['rejected'].append(c.decode())
    assert hashes=={'probe':sha(probe),'worker':sha(worker),'font':sha(font)}
    report['all_exact']=all(r['exact'] for r in report['cases']);a.output.write_text(json.dumps(report,indent=2)+'\n')
    if not report['all_exact']:raise SystemExit('not exact; diagnosis saved')
if __name__=='__main__': main()
