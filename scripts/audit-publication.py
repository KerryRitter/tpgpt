#!/usr/bin/env python3
"""Check the Git index (or tracked working tree) without printing secret values."""
import argparse
from pathlib import PurePosixPath
import re
import subprocess
import sys

ROOT_FILES = {
    '.gitignore', '.gitattributes', 'Cargo.toml', 'Cargo.lock', 'README.md',
    'package.json', 'package-lock.json', 'tsconfig.json',
}
SOURCE_ROOTS = {'src', 'test', 'scripts', 'docs', '.github'}
SOURCE_SUFFIXES = {'.rs', '.ts', '.js', '.py', '.sh', '.ps1', '.sql', '.md', '.yml', '.yaml', '.toml', '.json'}
PATTERNS = {
    'private key': re.compile(rb'-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----'),
    'GitHub credential': re.compile(rb'(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{50,})'),
    'provider credential': re.compile(rb'\bsk-(?:proj-|ant-api\d+-)?[A-Za-z0-9_-]{32,}'),
    'JWT credential': re.compile(rb'\beyJ[A-Za-z0-9_-]{15,}\.[A-Za-z0-9_-]{15,}\.[A-Za-z0-9_-]{15,}'),
    'literal bearer credential': re.compile(rb'Bearer [A-Za-z0-9_.+-]{40,}'),
    'personal home directory': re.compile(rb'(?:/home/|/Users/)(?!linuxbrew(?:/|\b)|runner(?:/|\b)|YOUR_USER(?:/|\b))[A-Za-z0-9._-]+/'),
    'Windows personal directory': re.compile(rb'[A-Za-z]:[\\/]Users[\\/](?!runneradmin[\\/]|YOUR_USER[\\/])[A-Za-z0-9._-]+[\\/]'),
}


def allowed(path):
    p = PurePosixPath(path)
    if path in ROOT_FILES:
        return True
    if path in {'native/Cargo.toml', 'native/README.md'}:
        return True
    return (p.parts[0] in SOURCE_ROOTS or path.startswith('native/src/')) and p.suffix in SOURCE_SUFFIXES


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--staged', action='store_true')
    args = parser.parse_args()
    entries = subprocess.check_output(['git', 'ls-files', '--stage', '-z']).split(b'\0')
    failures = []
    count = 0
    for entry in entries:
        if not entry:
            continue
        metadata, raw_path = entry.split(b'\t', 1)
        mode, object_id, stage = metadata.split()
        path = raw_path.decode('utf-8')
        count += 1
        if not allowed(path) or mode not in {b'100644', b'100755'} or stage != b'0':
            failures.append((path, 'not an allowed source file'))
            continue
        data = subprocess.check_output(['git', 'cat-file', 'blob', object_id]) if args.staged else open(path, 'rb').read()
        try:
            data.decode('utf-8')
        except UnicodeDecodeError:
            failures.append((path, 'binary or non-UTF-8 content'))
        if b'\0' in data:
            failures.append((path, 'binary content'))
        for name, pattern in PATTERNS.items():
            if pattern.search(data):
                failures.append((path, name))
    for path, reason in failures:
        print(f'REJECTED: {path}: {reason}', file=sys.stderr)
    if failures or not count:
        return 1
    print(f'Publication audit passed: {count} source files; no disallowed files or recognized credentials.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
