#!/usr/bin/env python3
"""Test whether rounded glyph coverage times a rounded clip mask is exact.

Hayro is only a read-only oracle here. This script does not enable native
fractional clipping or feed an oracle bitmap into the DynLex renderer.
"""
import argparse
import hashlib
import json
import random
import subprocess
import tempfile
from pathlib import Path

from audit_text_winansi import fixture


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument('--output', type=Path, required=True)
    args = cli.parse_args()
    probe = Path('target/release/backend_probe').resolve()
    research = Path('target/release/truetype_page_probe').resolve()
    font = Path('assets/fonts/LiberationSans-Regular.ttf')
    hashes = {'probe': sha(probe), 'research': sha(research), 'font': sha(font)}
    rng = random.Random(301)
    report = {'hashes': hashes, 'cases': [],
              'hypothesis': 'round(round(glyph_coverage * 255) * round(clip_coverage * 255) / 255)',
              'strip_mask_hypothesis': '(glyph_u8 * proxy_clip_u8 + 255) >> 8'}
    with tempfile.TemporaryDirectory(prefix='opds-fractional-clip-') as temp:
        root = Path(temp)
        for index in range(24):
            rotate = (0, 90, 180, 270)[index % 4]
            crop = ('0 0 640 800', '8 8 632 792')[index // 4 % 2]
            scale = (0.25, 1, 2)[index // 8]
            x, y = 35 + rng.random() * 40, 60 + rng.random() * 80
            width, height = 90 + rng.random() * 80, 50 + rng.random() * 60
            clip = f'{x:.7f} {y:.7f} {width:.7f} {height:.7f} re W n\n'.encode()
            text = b'BT /F 44 Tf 1 0 0 1 36 86 Tm (CLIP EDGE) Tj ET\n'
            full_black = b'0 0 640 800 re f\n'
            streams = (text, b'q\n' + clip + text + b'Q\n', b'q\n' + clip + full_black + b'Q\n')
            images = []
            source_hashes = []
            for number, stream in enumerate(streams):
                pdf = root / f'{number}.pdf'
                pdf.write_bytes(fixture(font.read_bytes(), stream, rotate, crop))
                source_hashes.append(sha(pdf))
                output = root / f'{number}.rgba'
                cmd = [str(probe), str(pdf), str(output), '--hayro', f'--scale={scale}']
                subprocess.run(cmd, check=True, capture_output=True, timeout=40)
                images.append(output.read_bytes())
                if number == 0:
                    own = root / 'dynlex.rgba'
                    cmd[2], cmd[3] = str(own), '--dynlex-strict'
                    subprocess.run(cmd, check=True, capture_output=True, timeout=40)
                    if own.read_bytes() != images[0]:
                        raise AssertionError('unclipped DynLex text is not pixel exact')
            unclipped, clipped, mask = images
            candidate_output = root / 'fractional.rgba'
            candidate = subprocess.run(
                [str(research), str(root / '1.pdf'), '1', str(scale),
                 str(candidate_output), '--fractional-research'],
                capture_output=True, text=True, timeout=40)
            candidate_pixels = candidate_output.read_bytes() if candidate.returncode == 0 else b''
            if len(unclipped) != len(clipped) or len(clipped) != len(mask):
                raise AssertionError('oracle dimensions disagree')
            mismatch = 0
            max_error = 0
            strip_mask_mismatch = 0
            strip_mask_max_error = 0
            samples = []
            for pixel in range(len(unclipped) // 4):
                at = pixel * 4
                glyph_alpha = 255 - unclipped[at]
                clip_alpha = 255 - mask[at]
                predicted = round(glyph_alpha * clip_alpha / 255)
                # vello_common 0.0.8 util.rs uses (a*b + 255) >> 8 for
                # intersecting two strip masks. The oracle's painted black
                # rectangle is only a proxy for its internal clip mask.
                strip_mask_predicted = (glyph_alpha * clip_alpha + 255) >> 8
                actual = 255 - clipped[at]
                if predicted != actual:
                    mismatch += 1
                    max_error = max(max_error, abs(predicted - actual))
                    if len(samples) < 4:
                        samples.append([pixel, glyph_alpha, clip_alpha, predicted, actual])
                if strip_mask_predicted != actual:
                    strip_mask_mismatch += 1
                    strip_mask_max_error = max(strip_mask_max_error, abs(strip_mask_predicted - actual))
            row = {'index': index, 'rotation': rotate, 'crop': crop, 'scale': scale,
                   'source_sha256': source_hashes, 'bytes': len(clipped),
                   'different_pixels': mismatch, 'max_coverage_error': max_error,
                   'strip_mask_different_pixels': strip_mask_mismatch,
                   'strip_mask_max_error': strip_mask_max_error,
                   'research_exit': candidate.returncode,
                   'research_error': candidate.stderr[-500:] if candidate.returncode else '',
                   'research_different_channels': (
                       sum(a != b for a, b in zip(candidate_pixels, clipped))
                       if len(candidate_pixels) == len(clipped) else None),
                   'samples': samples}
            report['cases'].append(row)
            print(index, rotate, crop, scale, mismatch, max_error, flush=True)
    assert hashes == {'probe': sha(probe), 'research': sha(research), 'font': sha(font)}
    report['exact_cases'] = sum(row['different_pixels'] == 0 for row in report['cases'])
    report['different_pixels'] = sum(row['different_pixels'] for row in report['cases'])
    report['strip_mask_exact_cases'] = sum(row['strip_mask_different_pixels'] == 0 for row in report['cases'])
    report['strip_mask_different_pixels'] = sum(row['strip_mask_different_pixels'] for row in report['cases'])
    report['research_exact_cases'] = sum(row['research_different_channels'] == 0 for row in report['cases'])
    args.output.write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
