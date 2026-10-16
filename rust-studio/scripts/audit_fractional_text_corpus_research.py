#!/usr/bin/env python3
"""Read-only, trusted-PDF audit of the separate fractional text candidate.

This intentionally bypasses native dispatch and the resource-limited worker.
Only exact candidate bitmaps count; unsupported pages retain no speed score.
"""
import argparse
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('pdf', type=Path)
    parser.add_argument('--pages', type=int, default=49)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if not 1 <= args.pages <= 500:
        raise ValueError('page count outside audit limit')
    probe = Path('target/release/backend_probe').resolve()
    research = Path('target/release/truetype_page_probe').resolve()
    report = {'pdf_sha256': sha(args.pdf), 'probe_sha256': sha(probe),
              'research_sha256': sha(research), 'cases': []}
    with tempfile.TemporaryDirectory(prefix='opds-fractional-corpus-') as temp:
        left, right = (Path(temp) / name for name in ('candidate.rgba', 'hayro.rgba'))
        for page in range(1, args.pages + 1):
            for scale in (0.25, 1, 2):
                left.unlink(missing_ok=True)
                candidate = subprocess.run(
                    [str(research), str(args.pdf), str(page), str(scale),
                     str(left), '--fractional-research'],
                    capture_output=True, text=True, timeout=40)
                row = {'page': page, 'scale': scale,
                       'candidate_exit': candidate.returncode}
                if candidate.returncode:
                    if left.exists():
                        raise AssertionError('rejected scene emitted bitmap')
                    row.update(status='unsupported', reason=candidate.stderr[-500:])
                else:
                    subprocess.run([str(probe), str(args.pdf), str(right),
                                    '--hayro', f'--scale={scale}',
                                    f'--page={page}'], check=True,
                                   capture_output=True, timeout=40)
                    a, b = left.read_bytes(), right.read_bytes()
                    row.update(status='exact' if a == b else 'mismatch',
                               rgba_bytes=len(a),
                               different_channels=sum(x != y for x, y in zip(a, b)))
                report['cases'].append(row)
            print(page, [r['status'] for r in report['cases'][-3:]], flush=True)
    if report['pdf_sha256'] != sha(args.pdf) or report['probe_sha256'] != sha(probe) or report['research_sha256'] != sha(research):
        raise AssertionError('audit input or binary changed')
    report['counts'] = {name: sum(r['status'] == name for r in report['cases'])
                        for name in ('exact', 'unsupported', 'mismatch')}
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    if report['counts']['mismatch']:
        raise SystemExit('research produced a mismatched bitmap; see report')


if __name__ == '__main__':
    main()
