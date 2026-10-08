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
    report = {'source': source['url'], 'sha256': digest, 'page': 1, 'scale': 1,
              'complete_original_document': True, 'cases': []}
    with tempfile.TemporaryDirectory(prefix='opds-real-jpeg-') as temporary:
        actual = None
        for mode in ['dynlex-strict', 'hayro']:
            out = Path(temporary) / (mode + '.rgba')
            result = subprocess.run([str(probe), str(pdf), str(out), '--' + mode, '--scale=1', '--page=1'], capture_output=True, text=True, timeout=120, check=True)
            data = out.read_bytes()
            if len(data) != 612 * 792 * 4:
                raise ValueError('unexpected bitmap extent')
            if mode == 'dynlex-strict':
                if 'PDF=Dynlex' not in result.stderr:
                    raise ValueError('unexpected renderer')
                actual = data
            elif actual != data:
                raise ValueError('full-page RGBA mismatch')
            report['cases'].append({'mode': mode, 'status': result.stderr.strip(), 'bytes': len(data), 'rgba_sha256': hashlib.sha256(data).hexdigest()})
        report['byte_equal'] = True
        # The image-scene zoom and text paths remain unproven and must not fall back.
        for page, scale in [(1, 0.25), (1, 2), (6, 1)]:
            out = Path(temporary) / 'unsupported.rgba'
            result = subprocess.run([str(probe), str(pdf), str(out), '--dynlex-strict', f'--scale={scale}', f'--page={page}'], capture_output=True, text=True, timeout=120)
            if result.returncode != 1 or out.exists() or 'No fallback rendered' not in result.stderr:
                raise ValueError('unexpected acceptance/fallback of unproven case')
        report['unsupported_without_output'] = [{'page': 1, 'scale': .25}, {'page': 1, 'scale': 2}, {'page': 6, 'scale': 1}]
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print('Full external page: 1,938,816 RGBA bytes exact; three unproven cases rejected')


if __name__ == '__main__':
    main()
