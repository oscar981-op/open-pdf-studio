#!/usr/bin/env python3
"""Require full-page equality through the native strict route at fractional scales."""
import argparse
import hashlib
import json
import re
import subprocess
import tempfile
from pathlib import Path
from diagnose_fractional_jpeg import SCALES
from audit_fractional_sampling import difference


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--backend', type=Path, required=True)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    source = next(x for x in json.loads(args.manifest.read_text()) if x['id'] == 'albany-contract')
    pdf = Path(source['path']).resolve(strict=True)
    if hashlib.sha256(pdf.read_bytes()).hexdigest() != source['sha256']:
        raise ValueError('source no longer matches pinned revision')
    backend = args.backend.resolve(strict=True)
    digest = hashlib.sha256(backend.read_bytes()).hexdigest()
    report = {'source': source['url'], 'source_sha256': source['sha256'],
              'backend_sha256': digest, 'complete_original_document': True,
              'page': 1, 'cases': []}
    with tempfile.TemporaryDirectory(prefix='opds-native-fractional-') as directory:
        for scale in SCALES:
            buffers = []
            for mode in ['dynlex-strict', 'hayro']:
                out = Path(directory) / mode
                result = subprocess.run([str(backend), str(pdf), str(out), '--' + mode,
                                         f'--scale={scale}'], capture_output=True, text=True,
                                        timeout=120, check=True)
                if mode == 'dynlex-strict' and 'PDF=Dynlex' not in result.stderr:
                    raise ValueError('unexpected renderer')
                buffers.append(out.read_bytes())
            width, height = map(int, re.search(r'(\d+)x(\d+)', result.stderr).groups())
            report['cases'].append({'scale': scale, 'width': width, 'height': height,
                'rgba_bytes': len(buffers[0]), 'rgba_sha256': hashlib.sha256(buffers[0]).hexdigest(),
                **difference(*buffers, width)})
    if hashlib.sha256(backend.read_bytes()).hexdigest() != digest:
        raise RuntimeError('backend changed during verification')
    report['all_exact'] = all(row.get('different_channels') == 0 for row in report['cases'])
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    if not report['all_exact']:
        raise SystemExit('native fractional sampling mismatch; diagnosis saved')
    print('20 whole original page scales exact through strict native DynLex route')


if __name__ == '__main__':
    main()
