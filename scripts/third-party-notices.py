#!/usr/bin/env python3
"""Collect public dependency license text; never include local paths or app data."""
import argparse
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--target', help='Target triple, defaulting to the current Rust host')
args = parser.parse_args()
host = args.target or next(line.split(': ', 1)[1] for line in subprocess.check_output(['rustc', '-vV'], text=True).splitlines() if line.startswith('host: '))
metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1', '--filter-platform', host]))
nodes = {n['id']: n for n in metadata['resolve']['nodes']}
wanted = set()
todo = list(metadata['workspace_members'])
while todo:
    package_id = todo.pop()
    if package_id in wanted:
        continue
    wanted.add(package_id)
    todo.extend(d['pkg'] for d in nodes[package_id]['deps'])
supplements = json.loads(Path('docs/third-party-license-supplements.json').read_text())
by_package = {name: group for group in supplements.values() for name in group['packages']}
parts = ['TPGPT: third-party dependency notices\n\nThese notices cover the Rust dependencies of the desktop build.\n']
count = 0
for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
    if not package.get('source') or package['id'] not in wanted:
        continue
    count += 1
    directory = Path(package['manifest_path']).parent
    license_files = sorted(p for p in directory.rglob('*') if p.is_file() and (p.name.lower().startswith(('license', 'licence', 'copying', 'notice')) or p.name.lower() in {'ofl.txt', 'ufl.txt'}))
    explicit = package.get('license_file')
    if explicit:
        p = directory / explicit
        if p.is_file() and p not in license_files:
            license_files.append(p)
    parts.append(f"\n{'=' * 72}\n{package['name']} {package['version']}\nLicense: {package.get('license') or 'See license text'}\n")
    if package.get('repository'):
        parts.append(f"Source: {package['repository']}\n")
    if package.get('authors'):
        parts.append(f"Authors: {', '.join(package['authors'])}\n")
    for license_file in license_files:
        parts.append(f'\n--- {license_file.relative_to(directory)} ---\n{license_file.read_text(errors="replace")}\n')
    supplement = by_package.get(f"{package['name']}@{package['version']}")
    if package['name'] == 'epaint_default_fonts':
        # The font files also require egui's code license, in addition to OFL/UFL.
        supplement = by_package[f"epaint@{package['version']}"]
    if supplement:
        for source in supplement['sources']:
            parts.append(f"\n--- Upstream license: {source['url']} ---\n{source.get('note', '')}\n{source['text']}\n")
    if not license_files and not supplement:
        raise SystemExit(f"Missing license text for {package['name']} {package['version']}; add an upstream supplement before packaging.")
destination = Path('native/bundle/THIRD-PARTY-NOTICES.txt')
destination.parent.mkdir(parents=True, exist_ok=True)
destination.write_text(''.join(parts), encoding='utf-8')
print(f'Generated dependency notices for {count} crates on {host}, including font licenses.')
