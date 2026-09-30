#!/usr/bin/env python3
"""Compare the standalone Rust probe with Poppler; QA-only Python dependencies."""
from __future__ import annotations
import json
import platform
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from PIL import Image, ImageChops, ImageStat

repo = Path(__file__).resolve().parents[2]
root = Path(__file__).resolve().parents[1]
probe = root / 'target/release/opds-engine-probe'
corpus = repo / 'test pdf-bestanden/Originele bestanden'
files = ['Tekst.pdf', 'Technische tekening.pdf',
         'Combinatie Raster, vector, tekening images.pdf', 'Zware vector PDF.pdf']
report = {'host': platform.platform(), 'rust_probe': str(probe), 'reference': 'pdftoppm at 72 dpi', 'files': []}
with tempfile.TemporaryDirectory(prefix='opds-engine-') as tmp:
    output = Path(tmp)
    for index, name in enumerate(files):
        source = corpus / name
        hayro_png = output / f'{index}-hayro.png'
        ref_png = output / f'{index}-poppler.png'
        entry = {'name': name, 'bytes': source.stat().st_size, 'page': 1}
        for label, command in [
            ('hayro', [str(probe), str(source), '1', str(hayro_png), '1']),
            ('poppler', ['pdftoppm', '-f', '1', '-l', '1', '-singlefile', '-cropbox', '-r', '72', '-png', str(source), str(ref_png.with_suffix(''))]),
        ]:
            started = time.monotonic()
            # GNU time writes maximum resident memory (KiB) to a separate file.
            memory = output / f'{index}-{label}.mem'
            try:
                result = subprocess.run(['/usr/bin/time', '-f', '%M', '-o', str(memory), *command],
                                        capture_output=True, text=True, timeout=75)
                entry[label] = {'exit': result.returncode, 'wall_ms': round((time.monotonic() - started) * 1000),
                                'max_rss_kib': int(memory.read_text().strip()) if memory.exists() else None,
                                'stdout': result.stdout.strip()[-500:], 'stderr': result.stderr.strip()[-500:]}
            except subprocess.TimeoutExpired:
                entry[label] = {'timeout_seconds': 75}
        if hayro_png.exists() and ref_png.exists():
            with Image.open(hayro_png) as hayro, Image.open(ref_png) as poppler:
                h, p = hayro.convert('RGB'), poppler.convert('RGB')
                entry['hayro']['dimensions'] = h.size
                entry['poppler']['dimensions'] = p.size
                compare_size = (min(h.width, p.width), min(h.height, p.height))
                difference = ImageChops.difference(h.crop((0, 0, *compare_size)), p.crop((0, 0, *compare_size)))
                entry['compared_dimensions'] = compare_size
                entry['mean_channel_difference'] = round(sum(ImageStat.Stat(difference).mean) / 3, 3)
                entry['different_pixels_percent'] = round(100 * sum(1 for px in difference.getdata() if any(px)) / (compare_size[0] * compare_size[1]), 2)
                # Preserve a small proof image per source for manual review.
                h.thumbnail((900, 900)); p.thumbnail((900, 900))
                h.save(root / 'docs' / f'probe-{index}-hayro.png')
                p.save(root / 'docs' / f'probe-{index}-poppler.png')
        report['files'].append(entry)
        print(json.dumps(entry, ensure_ascii=False), flush=True)
(root / 'docs' / 'engine-probe-results.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
