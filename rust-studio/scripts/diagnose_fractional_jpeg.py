#!/usr/bin/env python3
"""Replay independent fractional sampling candidates on the complete pinned scan."""
import argparse
import hashlib
import json
import re
import subprocess
import tempfile
from pathlib import Path

SCALES = [1.15, .87, 1.5625, .13, .21, .33, .47, .63, .77, .99,
          1.01, 1.13, 1.37, 1.49, 1.73, 1.99, 2.13, 2.47, 2.73, 3.01]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--candidate', type=Path, required=True)
    parser.add_argument('--oracle', type=Path, required=True)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    source = next(row for row in json.loads(args.manifest.read_text())
                  if row['id'] == 'albany-contract')
    pdf = Path(source['path']).resolve(strict=True)
    digest = hashlib.sha256(pdf.read_bytes()).hexdigest()
    if digest != source['sha256']:
        raise ValueError('source no longer matches pinned public PDF')
    candidate = args.candidate.resolve(strict=True)
    oracle = args.oracle.resolve(strict=True)
    report = {'source': source['url'], 'source_sha256': digest,
              'candidate_sha256': hashlib.sha256(candidate.read_bytes()).hexdigest(),
              'oracle_sha256': hashlib.sha256(oracle.read_bytes()).hexdigest(),
              'complete_original_document': True, 'page': 1,
              'normal_adapter_changed': False, 'cases': []}
    with tempfile.TemporaryDirectory(prefix='opds-fractional-jpeg-') as temp:
        root = Path(temp)
        for mode in ['geometric', 'strip-f32']:
            result = subprocess.run([str(candidate), str(pdf), str(root / mode),
                                     ','.join(map(str, SCALES)), mode],
                                    capture_output=True, text=True, timeout=120, check=True)
            # One original-source RGB decode reused for every scale in the process.
            sizes = re.findall(r'decoded_bytes=(\d+)', result.stdout)
            if len(sizes) != len(SCALES) or set(sizes) != {'100980000'}:
                raise ValueError('unexpected retained source buffer')
        for i, scale in enumerate(SCALES):
            out = root / 'oracle.rgba'
            result = subprocess.run([str(oracle), str(pdf), str(out), '--hayro',
                                     f'--scale={scale}'], capture_output=True, text=True,
                                    timeout=120, check=True)
            width, height = map(int, re.search(r'(\d+)x(\d+) Hayro', result.stderr).groups())
            expected = out.read_bytes()
            if len(expected) != width * height * 4:
                raise ValueError('invalid oracle dimensions')
            for mode in ['geometric', 'strip-f32']:
                actual = (root / mode / f'{i}.rgba').read_bytes()
                if len(actual) != len(expected):
                    raise ValueError('candidate dimensions differ')
                differences = [] if actual == expected else [
                    j for j, (a, b) in enumerate(zip(actual, expected)) if a != b]
                report['cases'].append({
                    'scale': scale, 'candidate': mode, 'width': width, 'height': height,
                    'rgba_bytes': len(actual),
                    'candidate_rgba_sha256': hashlib.sha256(actual).hexdigest(),
                    'oracle_rgba_sha256': hashlib.sha256(expected).hexdigest(),
                    'differing_channels': len(differences),
                    'differing_pixels': len({j // 4 for j in differences}),
                    'maximum_channel_error': max((abs(actual[j] - expected[j])
                                                  for j in differences), default=0),
                    'first_differing_channels': [
                        {'x': j // 4 % width, 'y': j // 4 // width, 'channel': j % 4,
                         'actual': actual[j], 'expected': expected[j]}
                        for j in differences[:24]],
                })
    report['strip_f32_all_exact'] = all(row['differing_channels'] == 0
        for row in report['cases'] if row['candidate'] == 'strip-f32')
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    if not report['strip_f32_all_exact']:
        raise SystemExit('strip candidate regression; per-pixel diagnosis saved')
    print('20 complete-page fractional zooms exact; native route remains unchanged')


if __name__ == '__main__':
    main()
