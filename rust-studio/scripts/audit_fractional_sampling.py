#!/usr/bin/env python3
"""Differential sampling audit; records mismatches, never relaxes renderer gates."""
import argparse
import hashlib
import io
import json
import re
import subprocess
import tempfile
from pathlib import Path
from PIL import Image

SCALES = [.431, .63, .87, .99999994, 1.00000012, 1.01,
          1.15, 1.3225, 1.37, 1.49, 1.5625, 1.73]


def make_jpeg():
    # High-frequency colour makes a one-source-pixel sampling error visible.
    image = Image.new('RGB', (1024, 1024))
    image.putdata([((x * 13 + y * 7) % 256, (x * 3 + y * 19) % 256,
                    (x * 17 + y * 11) % 256)
                   for y in range(1024) for x in range(1024)])
    out = io.BytesIO()
    image.save(out, format='JPEG', quality=90, subsampling=0, progressive=False)
    return out.getvalue()


def make_pdf(jpeg, rotation, crop, placement=(0, 0, 580, 400)):
    left, bottom, width, height = placement
    contents = f"q {width} 0 0 {height} {left} {bottom} cm /Scan Do Q".encode()
    boxes = f'/MediaBox [0 0 580 400] /CropBox [{" ".join(map(str, crop))}] /Rotate {rotation}'
    objects = [b'<< /Type /Catalog /Pages 2 0 R >>',
               ('<< /Type /Pages /Count 1 /Kids [3 0 R] ' + boxes +
                ' /Resources << /XObject << /Scan 5 0 R >> >> >>').encode(),
               b'<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>',
               f'<< /Length {len(contents)} >>\nstream\n'.encode() + contents + b'\nendstream',
               (f'<< /Type /XObject /Subtype /Image /Width 1024 /Height 1024 '
                f'/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode '
                f'/Length {len(jpeg)} >>\nstream\n').encode() + jpeg + b'\nendstream']
    data = bytearray(b'%PDF-1.7\n')
    offsets = [0]
    for i, obj in enumerate(objects, 1):
        offsets.append(len(data))
        data.extend(f'{i} 0 obj\n'.encode() + obj + b'\nendobj\n')
    xref = len(data)
    data.extend(f'xref\n0 {len(offsets)}\n0000000000 65535 f \n'.encode())
    for offset in offsets[1:]:
        data.extend(f'{offset:010d} 00000 n \n'.encode())
    data.extend(f'trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode())
    return data


def difference(actual, expected, width):
    if len(actual) != len(expected):
        return {'size_mismatch': [len(actual), len(expected)]}
    indices = [] if actual == expected else [i for i, (a, b) in
                                           enumerate(zip(actual, expected)) if a != b]
    return {'different_channels': len(indices),
            'different_pixels': len({i // 4 for i in indices}),
            'max_error': max((abs(actual[i] - expected[i]) for i in indices), default=0),
            'first_channels': [{'x': i // 4 % width, 'y': i // 4 // width,
                                'channel': i % 4, 'actual': actual[i], 'expected': expected[i]}
                               for i in indices[:8]]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--candidate', required=True, type=Path)
    parser.add_argument('--backend', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    candidate, backend = args.candidate.resolve(), args.backend.resolve()
    jpeg = make_jpeg()
    report = {'jpeg_sha256': hashlib.sha256(jpeg).hexdigest(),
              'candidate_sha256': hashlib.sha256(candidate.read_bytes()).hexdigest(),
              'backend_sha256': hashlib.sha256(backend.read_bytes()).hexdigest(),
              'cases': [], 'purpose': 'correctness audit, not timing benchmark'}
    with tempfile.TemporaryDirectory(prefix='opds-sampling-audit-') as temp:
        root = Path(temp)
        for rotation in [0, 90, 180, 270]:
            for crop in [[0, 0, 580, 400], [7, 11, 567, 389]]:
                pdf = root / 'source.pdf'
                pdf.write_bytes(make_pdf(jpeg, rotation, crop))
                subprocess.run([str(candidate), str(pdf), str(root / 'candidate'),
                                ','.join(map(str, SCALES)), 'strip-f32'],
                               capture_output=True, timeout=120, check=True)
                for i, scale in enumerate(SCALES):
                    oracle = root / 'hayro.rgba'
                    result = subprocess.run([str(backend), str(pdf), str(oracle), '--hayro',
                                             f'--scale={scale}'], capture_output=True,
                                            text=True, timeout=120, check=True)
                    width, height = map(int, re.search(r'(\d+)x(\d+) Hayro', result.stderr).groups())
                    expected = oracle.read_bytes()
                    row = {'rotation': rotation, 'crop': crop, 'scale': scale,
                           'width': width, 'height': height,
                           'pdf_sha256': hashlib.sha256(pdf.read_bytes()).hexdigest(),
                           'probe': difference((root / 'candidate' / f'{i}.rgba').read_bytes(),
                                               expected, width)}
                    native = root / 'native.rgba'
                    native.unlink(missing_ok=True)
                    result = subprocess.run([str(backend), str(pdf), str(native),
                                             '--dynlex-strict', f'--scale={scale}'],
                                            capture_output=True, text=True, timeout=120)
                    row['native_status'] = result.stderr.strip()
                    row['native'] = (difference(native.read_bytes(), expected, width)
                                     if result.returncode == 0 else {'unsupported': True})
                    report['cases'].append(row)
                print(f'checked rotation={rotation} crop={crop}', flush=True)
    for mode in ['probe', 'native']:
        report[mode + '_exact'] = sum(row[mode].get('different_channels') == 0
                                     for row in report['cases'])
        report[mode + '_unsupported'] = sum(row[mode].get('unsupported', False)
                                           for row in report['cases'])
    if hashlib.sha256(backend.read_bytes()).hexdigest() != report['backend_sha256']:
        raise RuntimeError('backend binary changed during audit; discard mixed results')
    if hashlib.sha256(candidate.read_bytes()).hexdigest() != report['candidate_sha256']:
        raise RuntimeError('candidate binary changed during audit; discard mixed results')
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print({k: v for k, v in report.items() if k.endswith(('_exact', '_unsupported'))})


if __name__ == '__main__':
    main()
