#!/usr/bin/env python3
"""Research-only per-paint fractional-clip PDF differential audit.

The candidate never runs through native dispatch or worker IPC. Hayro is
only the independent expected-pixel oracle.
"""
import argparse
import hashlib
import json
import random
import subprocess
import tempfile
from pathlib import Path

from audit_text_winansi import fixture


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--random-cases', type=int, default=96)
    parser.add_argument('--native', action='store_true',
                        help='audit strict DynLex dispatch instead of the separate research mode')
    args = parser.parse_args()
    if not 0 <= args.random_cases <= 1000:
        raise ValueError('random case count outside audit limit')
    probe = Path('target/release/backend_probe').resolve()
    research = Path('target/release/truetype_page_probe').resolve()
    font = Path('assets/fonts/LiberationSans-Regular.ttf')
    hashes = {p.name: sha(p) for p in (probe, research, font)}
    content = b'''0.14 0.27 0.53 rg 16 16 400 400 re f
q 40.25 100.5 400.5 300.25 re W n
0.8 0.1 0.2 rg 34 180 128 40 re f
BT /F 32 Tf 1 0 0 1 36 220 Tm (EDGE) Tj ET
q 80.125 140.375 300.25 200.125 re W* n
0.1 0.8 0.4 rg 76 160 160 40 re f
BT /F 30 Tf 1 0 0 1 72 260 Tm (NEST) Tj ET
Q
0.2 g BT /F 30 Tf 1 0 0 1 410 300 Tm (OUTER) Tj ET
Q
0 g BT /F 24 Tf 1 0 0 1 20 80 Tm (FREE) Tj ET
'''
    report = {'hashes': hashes, 'route': 'native' if args.native else 'research',
              'cases': []}
    def candidate_command(pdf, scale, output):
        if args.native:
            return [str(probe), str(pdf), str(output), '--dynlex-strict',
                    f'--scale={scale}']
        return [str(research), str(pdf), '1', str(scale),
                str(output), '--fractional-research']
    with tempfile.TemporaryDirectory(prefix='opds-fractional-research-') as temp:
        root = Path(temp)
        pdf, expected, actual = (root / name for name in
                                 ('page.pdf', 'hayro.rgba', 'candidate.rgba'))
        for rotation in (0, 90, 180, 270):
            for crop in ('0 0 640 800', '8 8 632 792'):
                pdf.write_bytes(fixture(font.read_bytes(), content, rotation, crop))
                for scale in (0.25, 1, 2):
                    subprocess.run([str(probe), str(pdf), str(expected),
                                    '--hayro', f'--scale={scale}'],
                                   check=True, capture_output=True, timeout=40)
                    actual.unlink(missing_ok=True)
                    run = subprocess.run(candidate_command(pdf, scale, actual),
                        capture_output=True, text=True, timeout=40)
                    row = {'kind': 'fixed', 'rotation': rotation, 'crop': crop, 'scale': scale,
                           'source_sha256': sha(pdf), 'exit': run.returncode}
                    if run.returncode == 0:
                        a, b = actual.read_bytes(), expected.read_bytes()
                        row.update(bytes=len(a),
                                   different_channels=sum(x != y for x, y in zip(a, b)),
                                   exact=a == b)
                    else:
                        row.update(exact=False, error=run.stderr[-1000:])
                    report['cases'].append(row)
                    print(rotation, crop, scale, row['exact'],
                          row.get('different_channels'), flush=True)
        rng = random.Random(431)
        for index in range(args.random_cases):
            rotation = (0, 90, 180, 270)[index % 4]
            crop = ('0 0 640 800', '8 8 632 792')[index // 4 % 2]
            scale = (0.25, 1, 2)[index // 8 % 3]
            x, y = 35 + rng.random() * 40, 100 + rng.random() * 60
            width, height = 320 + rng.random() * 60, 200 + rng.random() * 60
            stream = f'''0.14 0.27 0.53 rg 16 16 400 400 re f
q {x:.7f} {y:.7f} {width:.7f} {height:.7f} re W n
0.8 0.1 0.2 rg {x-4:.7f} {y+20:.7f} 128 40 re f
BT /F 32 Tf 1 0 0 1 {x-6:.7f} {y+35:.7f} Tm (EDGE) Tj ET
q {x+20:.7f} {y+25:.7f} {width-40:.7f} {height-50:.7f} re W* n
0.1 0.8 0.4 rg {x+18:.7f} {y+45:.7f} 160 40 re f
BT /F 30 Tf 1 0 0 1 {x+16:.7f} {y+65:.7f} Tm (NEST) Tj ET
Q Q
0 g BT /F 24 Tf 1 0 0 1 20 80 Tm (FREE) Tj ET
'''.encode()
            pdf.write_bytes(fixture(font.read_bytes(), stream, rotation, crop))
            subprocess.run([str(probe), str(pdf), str(expected),
                            '--hayro', f'--scale={scale}'],
                           check=True, capture_output=True, timeout=40)
            actual.unlink(missing_ok=True)
            run = subprocess.run(candidate_command(pdf, scale, actual),
                                 capture_output=True, text=True, timeout=40)
            row = {'kind': 'random', 'index': index, 'rotation': rotation,
                   'crop': crop, 'scale': scale, 'source_sha256': sha(pdf),
                   'exit': run.returncode}
            if run.returncode == 0:
                a, b = actual.read_bytes(), expected.read_bytes()
                row.update(bytes=len(a),
                           different_channels=sum(v != w for v, w in zip(a, b)),
                           exact=a == b)
            else:
                row.update(exact=False, error=run.stderr[-1000:])
            report['cases'].append(row)
            print('random', index, row['exact'],
                  row.get('different_channels'), flush=True)
    if hashes != {p.name: sha(p) for p in (probe, research, font)}:
        raise AssertionError('binary or font changed during audit')
    report['exact_cases'] = sum(x['exact'] for x in report['cases'])
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    if report['exact_cases'] != len(report['cases']):
        raise SystemExit('research mismatch or rejection; see report')


if __name__ == '__main__':
    main()
