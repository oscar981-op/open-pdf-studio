#!/usr/bin/env python3
"""Differential native DynLex text-clip audit; all accepted pixels must match Hayro."""
import argparse
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path

from audit_text_winansi import fixture


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    probe = Path('target/release/backend_probe').resolve()
    helper = probe.with_name('opds-text-worker')
    font = Path('assets/fonts/LiberationSans-Regular.ttf')
    hashes = {p.name: digest(p) for p in (probe, helper, font)}
    content = b'''0.18 0.40 0.7 rg 16 16 400 400 re f
q 64 64 256 256 re W n
0.85 0.10 0.18 rg 40 120 300 80 re f
BT /F 40 Tf 1 0 0 1 44 176 Tm (CLIPPED) Tj ET
q 128 128 128 128 re W* n
0.1 0.65 0.2 rg 112 156 200 44 re f
BT /F 28 Tf 1 0 0 1 116 188 Tm (nested) Tj ET
Q
0.2 0.2 0.75 rg BT /F 32 Tf 1 0 0 1 48 292 Tm (restored) Tj ET
Q
0.1 g BT /F 32 Tf 1 0 0 1 48 352 Tm (unclipped) Tj ET
'''
    report = {'hashes': hashes, 'cases': [], 'rejected': []}
    with tempfile.TemporaryDirectory(prefix='opds-text-clip-') as tmp:
        root = Path(tmp)
        for rotate in (0, 90, 180, 270):
            for crop in ('0 0 640 800', '8 8 632 792'):
                pdf = root / 'case.pdf'
                pdf.write_bytes(fixture(font.read_bytes(), content, rotate, crop))
                for scale in (0.25, 1, 2):
                    actual = root / 'actual.rgba'
                    expected = root / 'expected.rgba'
                    actual.unlink(missing_ok=True)
                    argv = [str(probe), str(pdf), str(actual), '--dynlex-strict', f'--scale={scale}']
                    run = subprocess.run(argv, capture_output=True, text=True, timeout=45)
                    row = {'rotate': rotate, 'crop': crop, 'scale': scale, 'source_sha256': digest(pdf), 'exit': run.returncode}
                    if run.returncode:
                        row['error'] = run.stderr[-1000:]
                        row['exact'] = False
                    else:
                        argv[2] = str(expected)
                        argv[3] = '--hayro'
                        subprocess.run(argv, capture_output=True, check=True, timeout=45)
                        left, right = actual.read_bytes(), expected.read_bytes()
                        row.update(bytes=len(left), differing_channels=sum(a != b for a, b in zip(left, right)), exact=left == right)
                    report['cases'].append(row)
                    print(rotate, crop, scale, row['exact'], row.get('differing_channels'), row.get('error', '')[-180:], flush=True)
        for bad in (b'640.25 64 20 20 re W n BT /F 12 Tf 1 0 0 1 64 100 Tm (X) Tj ET', b'64 64 256 256 re W', b'W n',
                    b'64 64 256 256 re W BT ET n', b'64 64 256 256 re n',
                    b'q 64 64 256 256 re W n'):
            pdf = root / 'bad.pdf'
            pdf.write_bytes(fixture(font.read_bytes(), b'BT /F 12 Tf 1 0 0 1 20 100 Tm (X) Tj ET\n' + bad))
            out = root / 'bad.rgba'
            out.unlink(missing_ok=True)
            run = subprocess.run([str(probe), str(pdf), str(out), '--dynlex-strict'], capture_output=True, timeout=45)
            if run.returncode == 0 or out.exists():
                raise AssertionError(f'accepted {bad!r}')
            report['rejected'].append(bad.decode())
    assert hashes == {p.name: digest(p) for p in (probe, helper, font)}
    report['all_exact'] = all(row['exact'] for row in report['cases'])
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    if not report['all_exact']:
        raise SystemExit('pixel mismatch or unsupported; see report')


if __name__ == '__main__':
    main()
