#!/usr/bin/env python3
"""Download the pinned public PDF corpus into a local cache, never into source control."""
import argparse
import hashlib
import json
import subprocess
import urllib.request
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--directory', type=Path, required=True)
    args = parser.parse_args()
    entries = json.loads(args.manifest.read_text())
    args.directory.mkdir(parents=True, exist_ok=True)
    local = []
    for entry in entries:
        name = entry['id']
        if not name or any(c not in 'abcdefghijklmnopqrstuvwxyz0123456789-' for c in name):
            raise ValueError('invalid corpus ID')
        path = args.directory.resolve() / (name + '.pdf')
        temporary = path.with_suffix('.part')
        if not path.exists():
            request = urllib.request.Request(entry['url'], headers={'User-Agent': 'OpenPDFStudioBenchmark/1.0'})
            try:
                with urllib.request.urlopen(request, timeout=60) as response, temporary.open('wb') as output:
                    size = 0
                    while chunk := response.read(1024 * 1024):
                        size += len(chunk)
                        if size > 100_000_000:
                            raise ValueError('100 MB download limit')
                        output.write(chunk)
                data = temporary.read_bytes()
                if not data[:1024].lstrip().startswith(b'%PDF-'):
                    raise ValueError('response is not a PDF')
                if hashlib.sha256(data).hexdigest() != entry['sha256']:
                    raise ValueError(f'{name}: public source changed; do not benchmark as the pinned revision')
                temporary.replace(path)
            finally:
                temporary.unlink(missing_ok=True)
        if path.stat().st_size > 100_000_000:
            raise ValueError('100 MB file limit')
        if hashlib.sha256(path.read_bytes()).hexdigest() != entry['sha256']:
            raise ValueError(f'{name}: cached file differs from manifest')
        subprocess.run(['pdfinfo', str(path)], check=True, capture_output=True, timeout=30)
        local.append(dict(entry, path=str(path)))
        print(f'{name}: verified {path.stat().st_size} bytes', flush=True)
    destination = args.directory / 'manifest.json'
    destination.write_text(json.dumps(local, indent=2) + '\n')
    print(f'Local benchmark manifest: {destination}')


if __name__ == '__main__':
    main()
