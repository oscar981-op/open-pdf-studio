#!/usr/bin/env python3
"""Compare one unchanged hybrid PDF page through strict native DynLex and Hayro."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

SCALES = (0.25, 0.87, 1.0, 1.15, 1.5625, 2.0)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pdf', type=Path, required=True)
    parser.add_argument('--page', type=int, required=True)
    parser.add_argument('--backend', type=Path, required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    worker = args.backend.with_name('opds-text-worker')
    original_hashes = {key: sha(path) for key, path in {
        'pdf': args.pdf, 'backend': args.backend, 'worker': worker}.items()}
    rows = []
    for scale in SCALES:
        actual = args.output_dir / f'page{args.page}-z{scale}.dynlex.rgba'
        expected = args.output_dir / f'page{args.page}-z{scale}.hayro.rgba'
        for path in (actual, expected):
            path.unlink(missing_ok=True)
        base = [str(args.backend), str(args.pdf)]
        tail = [f'--scale={scale}', f'--page={args.page}']
        native = subprocess.run(base + [str(actual), '--dynlex-strict'] + tail,
                                capture_output=True, text=True, timeout=120)
        oracle = subprocess.run(base + [str(expected), '--hayro'] + tail,
                                capture_output=True, text=True, timeout=120)
        row = {'scale': scale, 'dynlex_exit': native.returncode,
               'hayro_exit': oracle.returncode,
               'dynlex_diagnostic': native.stderr.strip()}
        if native.returncode == 0 and oracle.returncode == 0:
            left, right = actual.read_bytes(), expected.read_bytes()
            differences = [i for i, (a, b) in enumerate(zip(left, right)) if a != b]
            row.update(rgba_bytes=len(left), same_length=len(left) == len(right),
                       different_channels=len(differences),
                       different_pixels=len({i // 4 for i in differences}),
                       dynlex_sha256=sha(actual), hayro_sha256=sha(expected))
        elif actual.exists():
            raise RuntimeError('strict DynLex left a partial bitmap')
        print(scale, row.get('different_channels', row['dynlex_diagnostic']), flush=True)
        rows.append(row)
    if original_hashes != {key: sha(path) for key, path in {
            'pdf': args.pdf, 'backend': args.backend, 'worker': worker}.items()}:
        raise RuntimeError('binary or PDF changed during audit')
    report = {'scope': 'unchanged original page via native strict dispatch',
              'source': str(args.pdf), 'page': args.page,
              'sha256': original_hashes, 'cases': rows}
    path = args.output_dir / 'native-hybrid-page-audit.json'
    path.write_text(json.dumps(report, indent=2) + '\n')
    if any(row.get('different_channels') != 0 or not row.get('same_length')
           or row['dynlex_exit'] != 0 or row['hayro_exit'] != 0 for row in rows):
        raise SystemExit('native hybrid page differs or was rejected')


if __name__ == '__main__':
    main()
