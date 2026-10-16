#!/usr/bin/env python3
"""Differential audit for fractionally bounded clips proven inactive per paint."""
import argparse
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path

from audit_text_winansi import fixture


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    probe = Path('target/release/backend_probe').resolve()
    helper = probe.with_name('opds-text-worker')
    font = Path('assets/fonts/LiberationSans-Regular.ttf')
    hashes = {p.name: sha(p) for p in (probe, helper, font)}
    content = b'''0.12 0.34 0.64 rg 16 16 400 400 re f
q 40.25 100.5 400.5 300.25 re W n
q 80.125 140.375 300.25 200.125 re W* n
0.8 0.1 0.2 rg 96 180 128 40 re f
BT /F 32 Tf 1 0 0 1 100 220 Tm (SAFE) Tj ET
0.1 0.9 0.3 rg 2 2 12 12 re f
BT /F 28 Tf 1 0 0 1 520 520 Tm (DROP) Tj ET
Q
0.2 g BT /F 30 Tf 1 0 0 1 60 300 Tm (OUTER) Tj ET
Q
0 g BT /F 24 Tf 1 0 0 1 20 80 Tm (FREE) Tj ET
'''
    report = {'hashes': hashes, 'cases': [], 'active_cases': []}
    with tempfile.TemporaryDirectory(prefix='opds-inactive-fractional-') as temp:
        root = Path(temp)
        for rotation in (0, 90, 180, 270):
            for crop in ('0 0 640 800', '8 8 632 792'):
                pdf = root / 'page.pdf'
                pdf.write_bytes(fixture(font.read_bytes(), content, rotation, crop))
                for scale in (0.25, 1, 2):
                    actual, expected = root / 'dynlex.rgba', root / 'hayro.rgba'
                    actual.unlink(missing_ok=True)
                    call = [str(probe), str(pdf), str(actual), '--dynlex-strict', f'--scale={scale}']
                    run = subprocess.run(call, capture_output=True, text=True, timeout=40)
                    row = {'rotation': rotation, 'crop': crop, 'scale': scale,
                           'source_sha256': sha(pdf), 'exit': run.returncode}
                    if run.returncode:
                        row['error'] = run.stderr[-1000:]
                        row['exact'] = False
                    else:
                        call[2], call[3] = str(expected), '--hayro'
                        subprocess.run(call, check=True, capture_output=True, timeout=40)
                        left, right = actual.read_bytes(), expected.read_bytes()
                        row.update(bytes=len(left), different_channels=sum(a != b for a, b in zip(left, right)), exact=left == right)
                    report['cases'].append(row)
                    print(rotation, crop, scale, row['exact'], row.get('different_channels'), row.get('error', '')[-90:], flush=True)
                active = b'q 40.25 100.5 400.5 300.25 re W n BT /F 32 Tf 1 0 0 1 36 220 Tm (EDGE) Tj ET Q'
                pdf.write_bytes(fixture(font.read_bytes(), active, rotation, crop))
                out = root / 'active.rgba'
                out.unlink(missing_ok=True)
                run = subprocess.run([str(probe), str(pdf), str(out), '--dynlex-strict'], capture_output=True, timeout=40)
                if run.returncode or not out.exists():
                    raise AssertionError('active fractional glyph clip was rejected')
                expected = root / 'active-hayro.rgba'
                subprocess.run([str(probe), str(pdf), str(expected), '--hayro'],
                               check=True, capture_output=True, timeout=40)
                left, right = out.read_bytes(), expected.read_bytes()
                report['active_cases'].append({
                    'rotation': rotation, 'crop': crop,
                    'source_sha256': sha(pdf), 'bytes': len(left),
                    'different_channels': sum(a != b for a, b in zip(left, right)),
                    'exact': left == right})
    assert hashes == {p.name: sha(p) for p in (probe, helper, font)}
    report['all_exact'] = all(row['exact'] for row in report['cases'] + report['active_cases'])
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    if not report['all_exact']:
        raise SystemExit('unsupported/mismatch in an inactive case; see report')


if __name__ == '__main__':
    main()
