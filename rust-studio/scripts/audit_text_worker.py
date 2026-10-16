#!/usr/bin/env python3
"""Pixel-audit a resource-bounded Rust text worker and revalidated DynLex scene.
The native dispatcher remains unchanged. Hayro is an external test oracle only.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import zlib
from audit_text_winding import text_fixture
from audit_prepared_text import compare


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(cmd):
    return subprocess.run(list(map(str, cmd)), capture_output=True, text=True, timeout=30)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--bin-dir', type=Path, default=Path('target/release'))
    args = p.parse_args()
    bins = {n: (args.bin_dir / n).resolve(strict=True) for n in ['text_worker_probe', 'truetype_page_probe', 'backend_probe']}
    hashes = {n: sha(path) for n, path in bins.items()}
    source = next(x for x in json.loads(Path('docs/public-pdf-corpus-2026-10-08.json').read_text()) if x['id'] == 'albany-contract')
    pdf = Path(source['path'])
    assert sha(pdf) == source['sha256']
    font = Path('assets/fonts/LiberationSans-Regular.ttf')
    report = {'source_sha256': sha(pdf), 'binary_sha256': hashes, 'font_sha256': sha(font),
              'native_integration': False, 'worker_limits': {'address_bytes': 512*1024*1024, 'cpu_seconds': 6, 'wall_seconds': 10, 'scene_bytes': 32*1024*1024, 'stderr_bytes': 65536, 'global_concurrency': 1},
              'cases': [], 'rejections': [], 'timing_scope': 'Unpaired diagnostics only: process startup, PDF/font/geometry preparation and scene IPC; rendering uses fresh parent-owned buffers.'}
    with tempfile.TemporaryDirectory(prefix='opds-worker-audit-') as tmp:
        root = Path(tmp)
        cases = [(f'albany-{s}', pdf, 6, s) for s in [0.25, 0.5, 0.87, 1, 1.15, 1.5625, 2, 3]]
        for rotation in [0, 90, 180, 270]:
            for i, crop in enumerate(['0 0 640 800', '8 8 632 792']):
                path = root / f'fixture-{rotation}-{i}.pdf'
                path.write_bytes(text_fixture(font.read_bytes(), rotation, crop))
                cases.append((path.stem, path, 1, 1))
        for name, path, page, scale in cases:
            actual, expected = root/f'{name}.rgba', root/f'{name}.hayro'
            result = run([bins['text_worker_probe'], path, page, scale, actual])
            row = {'case': name, 'source_sha256': sha(path), 'page': page, 'scale': scale, 'exit_code': result.returncode, 'diagnostic': result.stderr.strip()}
            if result.returncode == 0:
                oracle = run([bins['backend_probe'], path, expected, '--hayro', f'--scale={scale}', f'--page={page}'])
                if oracle.returncode:
                    raise RuntimeError(oracle.stderr)
                row['pixels'] = compare(actual, expected)
                row['metrics'] = json.loads(result.stderr)
            elif actual.exists():
                raise RuntimeError('partial output after rejection')
            report['cases'].append(row)
            print(name, row.get('pixels', row['diagnostic']), flush=True)
        packed = zlib.compress(b' ' * (4*1024*1024+1))
        bomb = b'%PDF-1.7\n1 0 obj << /Type /ObjStm /N 1 /First 4 /Filter /FlateDecode /Length '+str(len(packed)).encode()+b' >>\nstream\n'+packed+b'\nendstream\nendobj'
        bad = [('objectstream-bomb', bomb, 1, '1'), ('truncated', pdf.read_bytes()[:200], 6, '1'),
               ('xref-offset', b'%PDF-1.7\nxref\n0 999999999\nstartxref\n999999999\n%%EOF', 1, '1'),
               ('nan-scale', pdf.read_bytes(), 6, 'NaN'), ('page-zero', pdf.read_bytes(), 0, '1')]
        for name, data, page, scale in bad:
            path, output = root/f'{name}.pdf', root/f'{name}.rgba'
            path.write_bytes(data)
            result = run([bins['text_worker_probe'], path, page, scale, output])
            report['rejections'].append({'case': name, 'rejected_without_output': result.returncode != 0 and not output.exists(), 'diagnostic': result.stderr.strip()})
        output = root/'native.rgba'
        result = run([bins['backend_probe'], pdf, output, '--dynlex-strict', '--scale=1', '--page=6'])
        report['native_still_explicitly_unsupported'] = result.returncode != 0 and not output.exists() and 'does not support' in result.stderr
        report['native_text_exact'] = False
        if result.returncode == 0:
            reference = root/'native-reference.rgba'
            oracle = run([bins['backend_probe'], pdf, reference, '--hayro', '--scale=1', '--page=6'])
            report['native_text_exact'] = oracle.returncode == 0 and 'PDF=Dynlex' in result.stderr and output.read_bytes() == reference.read_bytes()
    assert hashes == {n: sha(path) for n, path in bins.items()}, 'binary changed during audit'
    report['all_exact'] = all(x.get('pixels', {}).get('exact') for x in report['cases'])
    report['all_rejected'] = all(x['rejected_without_output'] for x in report['rejections'])
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    if not report['all_exact'] or not report['all_rejected'] or not (report['native_still_explicitly_unsupported'] or report['native_text_exact']):
        raise SystemExit('worker audit failed; report preserved')


if __name__ == '__main__':
    main()
