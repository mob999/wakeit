#!/usr/bin/env python3
"""Validate curated bilingual notes and extract a single release's English body."""
import argparse
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parents[1]
HEADING = re.compile(r'^## (\d+\.\d+\.\d+)\s*$', re.MULTILINE)


def entries(text):
    headings = list(HEADING.finditer(text))
    if not headings or text[:headings[0].start()].strip():
        raise ValueError('Changelog must begin with a version heading: ## X.Y.Z')
    result = {}
    for index, match in enumerate(headings):
        version = match.group(1)
        end = headings[index + 1].start() if index + 1 < len(headings) else len(text)
        body = text[match.end():end].strip()
        if version in result or not body:
            raise ValueError('Version entries must be unique and nonempty')
        if any(line.startswith('## ') for line in body.splitlines()):
            raise ValueError('Invalid version heading')
        result[version] = body
    versions = list(result)
    if versions != sorted(versions, key=lambda v: tuple(map(int, v.split('.'))), reverse=True):
        raise ValueError('Changelog versions must be newest first')
    return result


def validate(english, chinese, current):
    en, zh = entries(english), entries(chinese)
    if list(en) != list(zh):
        raise ValueError('English and Chinese version headings must match')
    if next(iter(en)) != current:
        raise ValueError('The current package version must be the first entry')
    return en


def product_version(product):
    manifest = ROOT / 'crates' / f'wakeit-{product}' / 'Cargo.toml'
    return tomllib.loads(manifest.read_text(encoding='utf-8'))['package']['version']


def changelog_paths(product):
    return (ROOT / 'CHANGELOG.cli.md', ROOT / 'CHANGELOG.cli.zh-CN.md') if product == 'cli' else (ROOT / 'CHANGELOG.md', ROOT / 'CHANGELOG.zh-CN.md')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--product', choices=('cli', 'gateway'), help='Product to extract; omit to validate both')
    parser.add_argument('--version', help='Extract this version, otherwise validate only')
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    products = (args.product,) if args.product else ('cli', 'gateway')
    if args.version and not args.product:
        parser.error('--version requires --product')
    for product in products:
        current = product_version(product)
        english, chinese = changelog_paths(product)
        notes = validate(english.read_text(encoding='utf-8'),
                         chinese.read_text(encoding='utf-8'), current)
    if args.version:
        if args.version not in notes:
            raise SystemExit('Requested version is absent from changelog')
        text = notes[args.version] + '\n'
        if args.output:
            args.output.write_text(text, encoding='utf-8')
        else:
            print(text, end='')
    elif args.output:
        parser.error('--output requires --version')


if __name__ == '__main__':
    main()
