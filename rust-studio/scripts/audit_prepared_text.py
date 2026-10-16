#!/usr/bin/env python3
"""Compare prepared DynLex text pages to independent complete Hayro buffers.
The PDF frontend remains research-only. Replay timings exclude parsing/staging,
file I/O and comparisons; both compositors consume identical prepared geometry.
"""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import tempfile

from audit_text_winding import text_fixture


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(command):
    return subprocess.run(list(map(str, command)), capture_output=True, text=True, timeout=120)


def compare(actual, expected):
    a, b = actual.read_bytes(), expected.read_bytes()
    if len(a) != len(b):
        return {'lengths': [len(a), len(b)], 'exact': False}
    if a == b:
        return {'bytes': len(a), 'different_channels': 0, 'exact': True}
    count, maximum = 0, 0
    for x, y in zip(a, b):
        if x != y:
            count += 1
            maximum = max(maximum, abs(x-y))
    return {'bytes': len(a), 'different_channels': count, 'maximum_error': maximum, 'exact': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, default=Path('target/release'))
    parser.add_argument('--manifest', type=Path, default=Path('docs/public-pdf-corpus-2026-10-08.json'))
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    binaries = {n: (args.bin_dir/n).resolve(strict=True) for n in ['truetype_page_probe', 'backend_probe']}
    before = {n: sha(p) for n, p in binaries.items()}
    source = next(x for x in json.loads(args.manifest.read_text()) if x['id'] == 'albany-contract')
    pdf = Path(source['path'])
    if sha(pdf) != source['sha256']:
        raise RuntimeError('source hash changed')
    font_path = Path('assets/fonts/LiberationSans-Regular.ttf')
    report = {'native_integration': False, 'source_sha256': sha(pdf), 'font_sha256': sha(font_path),
              'binary_sha256': before, 'cases': [],
              'timing_scope': 'Same prepared geometry, fresh output buffers; three alternating sequential/batch replay pairs. No PDF parsing, outline preparation, I/O or comparison in replay timers.'}
    with tempfile.TemporaryDirectory(prefix='opds-prepared-text-') as temp:
        root = Path(temp)
        cases = [(f'albany-{s}', pdf, 6, s) for s in [0.25, 0.5, 0.87, 1, 1.15, 1.5625, 2, 3]]
        for rotation in [0, 90, 180, 270]:
            for crop_index, crop in enumerate(['0 0 640 800', '8 8 632 792']):
                p = root / f'fixture-{rotation}-{crop_index}.pdf'
                p.write_bytes(text_fixture(font_path.read_bytes(), rotation, crop))
                cases.append((p.stem, p, 1, 1))
        for name, path, page, scale in cases:
            folder = root / name
            folder.mkdir()
            actual, expected = folder/'actual.rgba', folder/'expected.rgba'
            result = run([binaries['truetype_page_probe'], path, page, scale, actual,
                          '--prepared-winding', folder])
            row = {'case': name, 'page': page, 'scale': scale, 'source_sha256': sha(path),
                   'exit_code': result.returncode, 'diagnostic': result.stderr.strip()}
            if result.returncode == 0:
                oracle = run([binaries['backend_probe'], path, expected, '--hayro', f'--scale={scale}', f'--page={page}'])
                if oracle.returncode:
                    raise RuntimeError(oracle.stderr)
                row['pixels'] = compare(actual, expected)
                row['metrics'] = json.loads((folder/'prepared.json').read_text())
                metrics = row['metrics']
                row['median_replay_speedup'] = statistics.median(metrics['sequential_ms']) / statistics.median(metrics['replay_ms'])
            elif actual.exists():
                raise RuntimeError('partial output after rejection')
            report['cases'].append(row)
            print(name, row.get('pixels', {'accepted': False}), flush=True)
        # Existing strict native route must not silently acquire this research parser.
        output = root/'native.rgba'
        result = run([binaries['backend_probe'], pdf, output, '--dynlex-strict', '--scale=1', '--page=6'])
        report['native_still_explicitly_unsupported'] = result.returncode != 0 and not output.exists() and 'does not support' in result.stderr
        report['native_text_exact'] = False
        if result.returncode == 0:
            reference = root/'native-reference.rgba'
            oracle = run([binaries['backend_probe'], pdf, reference, '--hayro', '--scale=1', '--page=6'])
            report['native_text_exact'] = oracle.returncode == 0 and 'PDF=Dynlex' in result.stderr and output.read_bytes() == reference.read_bytes()
    if before != {n: sha(p) for n,p in binaries.items()}:
        raise RuntimeError('binaries changed during audit')
    report['all_exact'] = all(x.get('pixels', {}).get('exact') for x in report['cases'])
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    if not report['all_exact'] or not (report['native_still_explicitly_unsupported'] or report['native_text_exact']):
        raise SystemExit('audit failed; report preserved')


if __name__ == '__main__':
    main()
