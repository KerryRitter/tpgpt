#!/usr/bin/env python3
"""Collect public dependency license text; never include local paths or app data."""
import json
from pathlib import Path
import subprocess

metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1']))
parts = ['TPGPT: third-party dependency notices\n\nThese notices cover the Rust dependencies of the desktop build.\n']
for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
    if not package.get('source'):
        continue
    directory = Path(package['manifest_path']).parent
    license_files = sorted({p for pattern in ['LICENSE*', 'LICENCE*', 'COPYING*', 'NOTICE*', 'license*', 'licence*'] for p in directory.glob(pattern) if p.is_file()})
    explicit = package.get('license_file')
    if explicit:
        p = directory / explicit
        if p.is_file() and p not in license_files:
            license_files.append(p)
    parts.append(f"\n{'=' * 72}\n{package['name']} {package['version']}\nLicense: {package.get('license') or 'See license text'}\n")
    if package.get('repository'):
        parts.append(f"Source: {package['repository']}\n")
    for license_file in license_files:
        parts.append(f'\n--- {license_file.name} ---\n{license_file.read_text(errors="replace")}\n')
    if not license_files:
        parts.append('No separate license text was included in this crate archive. See the stated license and source repository.\n')
destination = Path('native/bundle/THIRD-PARTY-NOTICES.txt')
destination.parent.mkdir(parents=True, exist_ok=True)
destination.write_text(''.join(parts), encoding='utf-8')
print(f'Generated dependency notices for {len(metadata["packages"]) - 1} crates.')
