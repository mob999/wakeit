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
    'aarch64-apple-darwin', 'x86_64-pc-windows-msvc',
)


def official_resources(binary, suffix):
    manifest = json.loads((ROOT / 'wakeit-plugin.json').read_text())
    if manifest['version'] != product_version('cli'):
        raise SystemExit('The bundled official package version must match the CLI release')
    definitions = [(ROOT / path, path) for path in manifest['wakeit']['agents'].values()]
    programs = []
    for path in manifest['wakeit']['commands'].values():
        name = Path(path).name + suffix
        source = binary.parent / name
        if not source.is_file():
            raise SystemExit(f'Build the native helper before packaging: {name}')
        programs.append((source, path + suffix))
    return definitions, programs


def stage(binary, output):
    definitions, programs = official_resources(binary, '.exe' if binary.suffix == '.exe' else '')
    output.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / 'wakeit-plugin.json', output / 'wakeit-plugin.json')
    for source, relative in definitions + programs:
        destination = output / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest='command', required=True)
    binary = commands.add_parser('binary')
    binary.add_argument('--product', choices=('cli', 'gateway'), required=True)
    binary.add_argument('--target', choices=TARGETS, required=True)
    binary.add_argument('--input', type=Path, required=True)
    binary.add_argument('--output', type=Path, required=True)
    development = commands.add_parser('stage')
    development.add_argument('--input', type=Path, required=True)
    development.add_argument('--output', type=Path, required=True)
    assemble = commands.add_parser('assemble')
    assemble.add_argument('--product', choices=('cli', 'gateway'), required=True)
    assemble.add_argument('--input', type=Path, required=True)
    assemble.add_argument('--output', type=Path, required=True)
    assemble.add_argument('--tag')
    args = parser.parse_args()
    if args.command == 'stage':
        stage(args.input, args.output)
        return
    version = product_version(args.product)
    binary_name = 'wakeit' if args.product == 'cli' else 'wakeit-gateway'
    args.output.mkdir(parents=True, exist_ok=True)
    if args.command == 'binary':
        if args.product == 'gateway':
            name = f'{binary_name}-{args.target}' + ('.exe' if 'windows' in args.target else '')
            shutil.copy2(args.input, args.output / name)
        else:
            suffix = '.exe' if 'windows' in args.target else ''
            definitions, programs = official_resources(args.input, suffix)
            with tarfile.open(args.output / f'wakeit-agents-{args.target}.tar.gz', 'w:gz') as archive:
                for path, name in programs:
                    archive.add(path, arcname=name, recursive=False)
            with tarfile.open(args.output / f'wakeit-{args.target}.tar.gz', 'w:gz') as archive:
                archive.add(args.input, arcname='wakeit' + suffix, recursive=False)
                prefix = f'wakeit-plugins/{version}/'
                archive.add(ROOT / 'wakeit-plugin.json', arcname=prefix + 'wakeit-plugin.json', recursive=False)
                for path, name in definitions + programs:
                    archive.add(path, arcname=prefix + name, recursive=False)
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
        name = f'{binary_name}-{target}' + ('.tar.gz' if args.product == 'cli' else ('.exe' if 'windows' in target else ''))
        matches = [path for path in args.input.rglob(name) if path.is_file()]
        if len(matches) != 1:
            raise SystemExit(f'Expected exactly one native artifact for {target}')
        dest = args.output / name
        shutil.copy2(matches[0], dest)
        digest = hashlib.sha256(dest.read_bytes()).hexdigest()
        assets[target] = {'name': name, 'sha256': digest}
        lines.append(f'{digest}  {name}')
    if args.product == 'cli':
        for target in TARGETS:
            name = f'wakeit-agents-{target}.tar.gz'
            matches = list(args.input.rglob(name))
            if len(matches) != 1:
                raise SystemExit(f'Expected exactly one official plugin artifact for {target}')
            shutil.copy2(matches[0], args.output / name)
            lines.append(f"{hashlib.sha256((args.output / name).read_bytes()).hexdigest()}  {name}")
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
