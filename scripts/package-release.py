#!/usr/bin/env python3
"""Package native binaries into the updater's release contract (stdlib only)."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import tarfile
import tomllib
from changelog import validate

ROOT = Path(__file__).resolve().parents[1]
TARGETS = (
    'x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu',
    'x86_64-apple-darwin', 'aarch64-apple-darwin', 'x86_64-pc-windows-msvc',
)


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest='command', required=True)
    binary = commands.add_parser('binary')
    binary.add_argument('--target', choices=TARGETS, required=True)
    binary.add_argument('--input', type=Path, required=True)
    binary.add_argument('--output', type=Path, required=True)
    assemble = commands.add_parser('assemble')
    assemble.add_argument('--input', type=Path, required=True)
    assemble.add_argument('--output', type=Path, required=True)
    assemble.add_argument('--tag')
    args = parser.parse_args()
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['workspace']['package']['version']
    args.output.mkdir(parents=True, exist_ok=True)
    if args.command == 'binary':
        name = f'wakeit-{args.target}' + ('.exe' if 'windows' in args.target else '')
        shutil.copy2(args.input, args.output / name)
        return
    if args.tag:
        if not re.fullmatch(r'v\d+\.\d+\.\d+', args.tag) or args.tag != f'v{version}':
            raise SystemExit('Formal release tag must exactly match the package version')
    notes = validate((ROOT / 'CHANGELOG.md').read_text(encoding='utf-8'),
                     (ROOT / 'CHANGELOG.zh-CN.md').read_text(encoding='utf-8'), version)
    (args.output / 'release-notes.md').write_text(notes[version] + '\n', encoding='utf-8')
    assets = {}
    lines = []
    for target in TARGETS:
        name = f'wakeit-{target}' + ('.exe' if 'windows' in target else '')
        matches = [path for path in args.input.rglob(name) if path.is_file()]
        if len(matches) != 1:
            raise SystemExit(f'Expected exactly one native artifact for {target}')
        dest = args.output / name
        shutil.copy2(matches[0], dest)
        digest = hashlib.sha256(dest.read_bytes()).hexdigest()
        assets[target] = {'name': name, 'sha256': digest}
        lines.append(f'{digest}  {name}')
    with tarfile.open(args.output / 'wakeit-skill.tar.gz', 'w:gz') as archive:
        archive.add(ROOT / 'skills/wakeit', arcname='wakeit', filter=lambda item: None if '__pycache__' in item.name else item)
    lines.append(f"{hashlib.sha256((args.output / 'wakeit-skill.tar.gz').read_bytes()).hexdigest()}  wakeit-skill.tar.gz")
    shutil.copy2(ROOT / 'install.sh', args.output / 'install.sh')
    lines.append(f"{hashlib.sha256((args.output / 'install.sh').read_bytes()).hexdigest()}  install.sh")
    (args.output / 'wakeit-release.json').write_text(json.dumps({'version': version, 'assets': assets}, indent=2) + '\n', encoding='utf-8')
    (args.output / 'SHA256SUMS').write_text('\n'.join(lines) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()
