#!/usr/bin/env python3
"""Fail closed on known native links; list build scripts for manual review.

This checks the resolved graph, not source-code behavior. Re-review on each lockfile change.
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
nodes = {node['id']: node for node in data['resolve']['nodes']}
roots = [package['id'] for package in packages.values() if package['name'] in
         {'opds-native-core', 'opds-engine-probe'}]
visited = set()
remaining = roots[:]
while remaining:
    item = remaining.pop()
    if item in visited:
        continue
    visited.add(item)
    remaining.extend(nodes[item]['dependencies'])
forbidden = ('pdfium', 'tesseract', 'turbojpeg', 'mozjpeg', 'openssl',
             'freetype', 'harfbuzz', 'mu-pdf', 'mupdf', 'tauri')
problems = []
build_scripts = []
for package_id in visited:
    package = packages[package_id]
    name = package['name']
    if any(piece in name.lower() for piece in forbidden):
        problems.append(f'forbidden crate: {name}')
    if package.get('links'):
        problems.append(f'native link target: {name} -> {package["links"]}')
    if any('custom-build' in target['kind'] for target in package['targets']):
        build_scripts.append(name)
print(f'Resolved packages: {len(visited)}')
print('Build scripts requiring source review: ' + ', '.join(sorted(build_scripts)))
for problem in problems:
    print(problem, file=sys.stderr)
if problems:
    sys.exit(1)
print('Known native-link/forbidden-crate gate: PASS')
