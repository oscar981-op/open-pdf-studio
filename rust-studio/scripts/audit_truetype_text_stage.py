#!/usr/bin/env python3
"""Compare a complete original text page's staging with a test-only Hayro oracle.

This checks glyph identities/positions, NOT pixel parity or native PDF support.
The candidate must either write a complete RGBA buffer or reject without output.
"""
import argparse
import ast
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(args):
    return subprocess.run(list(map(str, args)), capture_output=True, text=True, timeout=120)


def compare(staged, oracle):
    rows = [s.split('\t') for s in staged.splitlines()]
    upem = {r[1]: float(r[2]) for r in rows if r[0] == 'F'}
    glyphs = []
    paints = []
    for r in rows:
        if len(r) > 1 and r[1] == 'G':
            matrix = ast.literal_eval(r[4])
            matrix[:4] = [v * upem[r[2]] / 1000 for v in matrix[:4]]
            glyphs.append((int(r[3]), matrix))
            paints.append('G')
        elif len(r) > 1 and r[1] == 'R':
            paints.append('R')
    expected = [r.split('\t') for r in oracle.splitlines()]
    oracle_glyphs = [(int(r[1]), ast.literal_eval(r[2])) for r in expected if r[0] == 'G']
    if len(glyphs) != len(oracle_glyphs):
        raise ValueError('glyph count mismatch')
    differences = [dict(index=i, gid=a[0], oracle_gid=b[0],
                        delta=[x-y for x, y in zip(a[1], b[1])])
                   for i, (a, b) in enumerate(zip(glyphs, oracle_glyphs))]
    return dict(dimensions=list(map(int, rows[0][1:3])), codes=int(rows[0][3]),
                operators=int(rows[0][4]), fonts=len(upem), visible_glyphs=len(glyphs),
                rectangles=paints.count('R'), paint_order_exact=paints == [r[0] for r in expected],
                gid_mismatches=sum(r['gid'] != r['oracle_gid'] for r in differences),
                max_linear_delta=max(abs(v) for r in differences for v in r['delta'][:4]),
                max_position_delta=max(abs(v) for r in differences for v in r['delta'][4:]),
                nonzero_deltas=differences)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--candidate', type=Path, required=True)
    parser.add_argument('--oracle', type=Path, required=True)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    source = next(r for r in json.loads(args.manifest.read_text()) if r['id'] == 'albany-contract')
    pdf = Path(source['path']).resolve(strict=True)
    if digest(pdf) != source['sha256']:
        raise ValueError('PDF does not match pinned source')
    binaries = [args.candidate.resolve(strict=True), args.oracle.resolve(strict=True)]
    hashes = [digest(p) for p in binaries]
    report = dict(source=source['url'], source_sha256=digest(pdf), page=6,
                  candidate_sha256=hashes[0], oracle_sha256=hashes[1],
                  purpose='staging diagnosis only, no native support or pixel parity claim', cases=[])
    with tempfile.TemporaryDirectory(prefix='opds-text-stage-') as tmp:
        for scale in [0.25, 1.0, 2.0]:
            folder = Path(tmp) / str(scale)
            output = folder / 'page.rgba'
            trace = folder / 'oracle.tsv'
            folder.mkdir()
            p = run([binaries[0], pdf, 6, scale, output, '--stage-only', folder])
            if p.returncode or output.exists():
                raise RuntimeError(p.stderr or 'staging wrote unexpected bitmap')
            p = run([binaries[1], pdf, 6, scale, trace])
            if p.returncode:
                raise RuntimeError(p.stderr)
            candidate_trace = folder / 'staged.tsv'
            row = compare(candidate_trace.read_text(), trace.read_text())
            row.update(scale=scale, staged_sha256=digest(candidate_trace), oracle_sha256=digest(trace))
            p = run([binaries[0], pdf, 6, scale, output, '--diagnostics', folder])
            row['render_exit'] = p.returncode
            row['render_diagnostic'] = p.stderr.strip()
            row['rgba_written'] = output.exists()
            row['outline_diagnostics'] = [dict(file=f.name, bytes=f.stat().st_size, sha256=digest(f))
                                          for f in sorted(folder.glob('paint-*.opm'))]
            if p.returncode == 0 or output.exists():
                raise RuntimeError('render behavior changed: add complete Hayro pixel comparison before acceptance')
            report['cases'].append(row)
    if hashes != [digest(p) for p in binaries]:
        raise RuntimeError('binary changed during audit')
    report['staging_matches'] = all(r['gid_mismatches'] == 0 and r['paint_order_exact']
                                    and r['max_position_delta'] < 1e-9
                                    and r['max_linear_delta'] < 1e-9 for r in report['cases'])
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    if not report['staging_matches']:
        raise SystemExit('staging mismatch, diagnosis saved')
    print('3 complete-page text stages agree with oracle; rendering remains explicitly rejected')


if __name__ == '__main__':
    main()
