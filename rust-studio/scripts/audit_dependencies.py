#!/usr/bin/env python3
"""Audit the active Linux app graph for known non-Rust dependencies.

This checks package metadata, not source-code behavior. Re-review on each lockfile change.
"""
from __future__ import annotations
import json
import os
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[1]
command = [os.environ.get('CARGO', 'cargo'), 'metadata', '--locked', '--format-version', '1',
           '--manifest-path', str(root / 'Cargo.toml')]
data = json.loads(subprocess.check_output(command))
packages = {package['id']: package for package in data['packages']}
target = os.environ.get('OPDS_AUDIT_TARGET', 'x86_64-unknown-linux-gnu')
tree_command = [os.environ.get('CARGO', 'cargo'), 'tree', '--locked',
                '--manifest-path', str(root / 'Cargo.toml'), '-p', 'opds-native-app',
                '--target', target, '-e', 'normal,build', '--prefix', 'none',
                '--format', '{p}']
active = set()
for line in subprocess.check_output(tree_command, text=True).splitlines():
    parts = line.split()
    if len(parts) >= 2 and parts[1].startswith('v'):
        active.add((parts[0], parts[1][1:]))
visited = {package_id for package_id, package in packages.items()
           if (package['name'], package['version']) in active}
forbidden = ('pdfium', 'tesseract', 'turbojpeg', 'mozjpeg', 'openssl',
             'freetype', 'harfbuzz', 'mu-pdf', 'mupdf', 'tauri')
forbidden_exact = {'cc', 'cmake', 'bindgen'}
problems = []
build_scripts = []
for package_id in visited:
    package = packages[package_id]
    name = package['name']
    if name.lower() in forbidden_exact or any(piece in name.lower() for piece in forbidden):
        problems.append(f'forbidden crate: {name}')
    if package.get('links'):
        problems.append(f'native link target: {name} -> {package["links"]}')
    if any('custom-build' in target['kind'] for target in package['targets']):
        build_scripts.append(name)
print(f'Active packages for {target}: {len(visited)}')
print('Build scripts requiring source review: ' + ', '.join(sorted(set(build_scripts))))
for problem in problems:
    print(problem, file=sys.stderr)
if problems:
    sys.exit(1)
print('Known native-link/forbidden-crate gate: PASS')
