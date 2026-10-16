#!/usr/bin/env python3
"""Measure remaining placement/upsampling errors, without accepting average error."""
import argparse
import hashlib
import json
import re
import subprocess
import tempfile
from pathlib import Path
from audit_fractional_sampling import make_jpeg, make_pdf, difference


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--backend', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--candidate', type=Path)
    p.add_argument('--require-exact', action='store_true')
    args = p.parse_args()
    backend = args.backend.resolve(strict=True)
    digest = hashlib.sha256(backend.read_bytes()).hexdigest()
    candidate = args.candidate.resolve(strict=True) if args.candidate else None
    candidate_digest = hashlib.sha256(candidate.read_bytes()).hexdigest() if candidate else None
    jpeg = make_jpeg()
    cases = []
    with tempfile.TemporaryDirectory(prefix='opds-unverified-zoom-') as tmp:
        root = Path(tmp)
        for turn in [0, 90, 180, 270]:
            for place in [(0, 0, 580, 400), (16, 20, 548, 352)]:
                pdf = root / 'input.pdf'
                pdf.write_bytes(make_pdf(jpeg, turn, [7, 11, 567, 389], place))
                for scale in [.87, 1.15, 1.5625, 3.17, 5.]:
                    oracle = root / 'oracle.rgba'
                    r = subprocess.run([str(backend), str(pdf), str(oracle), '--hayro',
                                        f'--scale={scale}'], capture_output=True, text=True,
                                       timeout=120, check=True)
                    w, h = map(int, re.search(r'(\d+)x(\d+)', r.stderr).groups())
                    out = root / 'native.rgba'
                    out.unlink(missing_ok=True)
                    r = subprocess.run([str(backend), str(pdf), str(out), '--dynlex-strict',
                                        f'--scale={scale}'], capture_output=True, text=True, timeout=120)
                    row = {'turn': turn, 'placement': place, 'scale': scale,
                           'width': w, 'height': h, 'status': r.stderr.strip()}
                    if r.returncode:
                        if out.exists() or 'No fallback rendered' not in r.stderr:
                            raise ValueError('error must not emit a replacement bitmap')
                        row['unsupported'] = True
                    else:
                        if 'PDF=Dynlex' not in r.stderr:
                            raise ValueError('unexpected renderer')
                        row.update(difference(out.read_bytes(), oracle.read_bytes(), w))
                    if candidate:
                        subprocess.run([str(candidate), str(pdf), str(root / 'candidate'),
                                        str(scale), 'unverified-tiles'], capture_output=True,
                                       text=True, timeout=120, check=True)
                        row['diagnostic'] = difference((root / 'candidate/0.rgba').read_bytes(),
                                                       oracle.read_bytes(), w)
                    cases.append(row)
                print(f'checked turn={turn} placement={place}', flush=True)
    if hashlib.sha256(backend.read_bytes()).hexdigest() != digest:
        raise RuntimeError('binary changed during audit')
    report = {'backend_sha256': digest, 'jpeg_sha256': hashlib.sha256(jpeg).hexdigest(),
              'cases': cases, 'exact': sum(x.get('different_channels') == 0 for x in cases),
              'unsupported': sum(x.get('unsupported', False) for x in cases),
              'mismatched': sum(x.get('different_channels', 0) > 0 for x in cases)}
    if candidate:
        if hashlib.sha256(candidate.read_bytes()).hexdigest() != candidate_digest:
            raise RuntimeError('candidate changed during audit')
        report['candidate_sha256'] = candidate_digest
        report['diagnostic_exact'] = sum(x['diagnostic'].get('different_channels') == 0 for x in cases)
        report['diagnostic_mismatched'] = len(cases) - report['diagnostic_exact']
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print({k: v for k, v in report.items() if isinstance(v, int)})
    if args.require_exact and (report['mismatched'] or any('size_mismatch' in x for x in cases)):
        raise SystemExit('strict route emitted incorrect pixels')


if __name__ == '__main__':
    main()
