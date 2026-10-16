#!/usr/bin/env python3
"""Read-only oracle decomposition of the two remaining clip-proxy pixels.

The PDF fixture declares every WinAnsi width as 600/1000 em, so each
44-point character advances 26.4 PDF units. No oracle pixels enter DynLex.
"""
import argparse
import hashlib
import json
import math
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
    args = parser.parse_args()
    probe = Path('target/release/backend_probe').resolve()
    font = Path('assets/fonts/LiberationSans-Regular.ttf')
    rng = random.Random(301)
    for _ in range(24):
        x, y = 35 + rng.random() * 40, 60 + rng.random() * 80
        width, height = 90 + rng.random() * 80, 50 + rng.random() * 60
    clip = f'q {x:.7f} {y:.7f} {width:.7f} {height:.7f} re W n\n'.encode()
    spots = [(1376, 816), (1376, 817)]
    rows = [{'pixel': [px, py], 'glyphs': {}} for px, py in spots]
    report = {'probe_sha256': sha(probe), 'font_sha256': sha(font),
              'clip': [x, y, width, height], 'rows': rows}
    with tempfile.TemporaryDirectory(prefix='opds-fractional-overlap-') as temp:
        root = Path(temp)
        pdf = root / 'one.pdf'
        output = root / 'one.rgba'
        for index, char in enumerate('CLIP EDGE'):
            if char == ' ':
                continue
            position = 36 + index * 26.4
            for clipped in (False, True):
                stream = (clip if clipped else b'') + (
                    f'BT /F 44 Tf 1 0 0 1 {position:.7f} 86 Tm '
                    f'({char}) Tj ET\n'
                ).encode() + (b'Q\n' if clipped else b'')
                pdf.write_bytes(fixture(font.read_bytes(), stream, 270,
                                        '8 8 632 792'))
                subprocess.run([str(probe), str(pdf), str(output), '--hayro',
                                '--scale=2'], check=True, capture_output=True,
                               timeout=40)
                rgba = output.read_bytes()
                for row, (px, py) in zip(rows, spots):
                    coverage = 255 - rgba[(py * 1568 + px) * 4]
                    if coverage:
                        item = row['glyphs'].setdefault(f'{index}:{char}', {})
                        item['clipped' if clipped else 'bare'] = coverage
        full = clip + b'BT /F 44 Tf 1 0 0 1 36 86 Tm (CLIP EDGE) Tj ET\nQ\n'
        pdf.write_bytes(fixture(font.read_bytes(), full, 270,
                                '8 8 632 792'))
        subprocess.run([str(probe), str(pdf), str(output), '--hayro',
                        '--scale=2'], check=True, capture_output=True,
                       timeout=40)
        rgba = output.read_bytes()
        for row, (px, py) in zip(rows, spots):
            row['full_clipped_coverage'] = 255 - rgba[(py * 1568 + px) * 4]
            row['source_over_ideal'] = round(255 * (1 - math.prod(
                1 - part.get('clipped', 0) / 255
                for part in row['glyphs'].values())))
    if sha(probe) != report['probe_sha256'] or sha(font) != report['font_sha256']:
        raise AssertionError('oracle binary or font changed during audit')
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print([(row['pixel'], row['glyphs'], row['source_over_ideal'],
            row['full_clipped_coverage']) for row in rows])


if __name__ == '__main__':
    main()
