#!/usr/bin/env python3
"""Verify strict DynLex on the pinned full external scan, never an extracted page."""
import argparse
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--probe', type=Path, required=True)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    source = next(x for x in json.loads(args.manifest.read_text()) if x['id'] == 'albany-contract')
    pdf = Path(source['path']).resolve(strict=True)
    digest = hashlib.sha256(pdf.read_bytes()).hexdigest()
    if digest != source['sha256']:
        raise ValueError('public PDF differs from pinned revision')
    probe = args.probe.resolve(strict=True)
    report = {'source': source['url'], 'sha256': digest, 'page': 1, 'scales': [.25, .5, 1, 2],
              'complete_original_document': True, 'cases': []}
    with tempfile.TemporaryDirectory(prefix='opds-real-jpeg-') as temporary:
        for scale in [.25, .5, 1, 2]:
            actual = None
            for mode in ['dynlex-strict', 'hayro']:
                out = Path(temporary) / (mode + '.rgba')
                result = subprocess.run([str(probe), str(pdf), str(out), '--' + mode, f'--scale={scale}', '--page=1'], capture_output=True, text=True, timeout=120, check=True)
                data = out.read_bytes()
                if len(data) != int(612*scale) * int(792*scale) * 4:
                    raise ValueError('unexpected bitmap extent')
                if mode == 'dynlex-strict':
                    if 'PDF=Dynlex' not in result.stderr:
                        raise ValueError('unexpected renderer')
                    actual = data
                elif actual != data:
                    raise ValueError('full-page RGBA mismatch')
                report['cases'].append({'scale': scale, 'mode': mode, 'status': result.stderr.strip(), 'bytes': len(data), 'rgba_sha256': hashlib.sha256(data).hexdigest()})
        report['byte_equal'] = True
        # True font text and fractional image sampling remain unsupported.
        for page, scale in [(1, 0.3), (1, 1.25), (6, 1)]:
            out = Path(temporary) / 'unsupported.rgba'
            result = subprocess.run([str(probe), str(pdf), str(out), '--dynlex-strict', f'--scale={scale}', f'--page={page}'], capture_output=True, text=True, timeout=120)
            if result.returncode != 1 or out.exists() or 'No fallback rendered' not in result.stderr:
                raise ValueError('unexpected acceptance/fallback of unproven case')
        report['unsupported_without_output'] = [{'page': 1, 'scale': .3}, {'page': 1, 'scale': 1.25}, {'page': 6, 'scale': 1}]
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print('Full external page: 25/50/100/200% RGBA exact; fractional zoom and font text rejected')


if __name__ == '__main__':
    main()
