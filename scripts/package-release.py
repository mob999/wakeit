#!/usr/bin/env python3
"""Package native binaries into the updater's release contract (stdlib only)."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import tarfile
from changelog import validate, product_version, changelog_paths

ROOT = Path(__file__).resolve().parents[1]
TARGETS = (
    'x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu',
    'x86_64-apple-darwin', 'aarch64-apple-darwin', 'x86_64-pc-windows-msvc',
)


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest='command', required=True)
    binary = commands.add_parser('binary')
    binary.add_argument('--product', choices=('cli', 'gateway'), required=True)
    binary.add_argument('--target', choices=TARGETS, required=True)
    binary.add_argument('--input', type=Path, required=True)
    binary.add_argument('--output', type=Path, required=True)
    assemble = commands.add_parser('assemble')
    assemble.add_argument('--product', choices=('cli', 'gateway'), required=True)
    assemble.add_argument('--input', type=Path, required=True)
    assemble.add_argument('--output', type=Path, required=True)
    assemble.add_argument('--tag')
    args = parser.parse_args()
    version = product_version(args.product)
    binary_name = 'wakeit' if args.product == 'cli' else 'wakeit-gateway'
    args.output.mkdir(parents=True, exist_ok=True)
    if args.command == 'binary':
        name = f'{binary_name}-{args.target}' + ('.exe' if 'windows' in args.target else '')
        shutil.copy2(args.input, args.output / name)
        return
    if args.tag:
        if args.tag != f'{args.product}-v{version}' or not re.fullmatch(r'\d+\.\d+\.\d+', version):
            raise SystemExit('Formal release tag must exactly match the package version')
    english, chinese = changelog_paths(args.product)
    notes = validate(english.read_text(encoding='utf-8'), chinese.read_text(encoding='utf-8'), version)
    (args.output / 'release-notes.md').write_text(notes[version] + '\n', encoding='utf-8')
    assets = {}
    lines = []
    for target in TARGETS:
        name = f'{binary_name}-{target}' + ('.exe' if 'windows' in target else '')
        matches = [path for path in args.input.rglob(name) if path.is_file()]
        if len(matches) != 1:
            raise SystemExit(f'Expected exactly one native artifact for {target}')
        dest = args.output / name
        shutil.copy2(matches[0], dest)
        digest = hashlib.sha256(dest.read_bytes()).hexdigest()
        assets[target] = {'name': name, 'sha256': digest}
        lines.append(f'{digest}  {name}')
    if args.product == 'cli':
        with tarfile.open(args.output / 'wakeit-skill.tar.gz', 'w:gz') as archive:
            archive.add(ROOT / 'skills/wakeit', arcname='wakeit', filter=lambda item: None if '__pycache__' in item.name else item)
        lines.append(f"{hashlib.sha256((args.output / 'wakeit-skill.tar.gz').read_bytes()).hexdigest()}  wakeit-skill.tar.gz")
    installer = 'install.sh' if args.product == 'cli' else 'deploy-gateway.sh'
    shutil.copy2(ROOT / installer, args.output / installer)
    lines.append(f"{hashlib.sha256((args.output / installer).read_bytes()).hexdigest()}  {installer}")
    (args.output / f'wakeit-{args.product}-release.json').write_text(json.dumps({
        'product': args.product, 'version': version, 'assets': assets,
    }, indent=2) + '\n', encoding='utf-8')
    (args.output / 'SHA256SUMS').write_text('\n'.join(lines) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()
