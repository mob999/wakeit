#!/usr/bin/env python3
"""Enforce the public plugin dependency boundary on the current target."""
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / '.local/validation/sdk-boundary'


def dependency_tree(package):
    result = subprocess.run(
        ['cargo', 'tree', '--locked', '-p', package, '--edges', 'normal,build', '--prefix', 'none'],
        cwd=ROOT, check=True, text=True, encoding='utf-8', stdout=subprocess.PIPE,
    ).stdout
    (OUTPUT / f'{package}.txt').write_text(result, encoding='utf-8')
    return set(re.findall(r'^([a-zA-Z0-9_-]+) v', result, re.MULTILINE))


def main():
    OUTPUT.mkdir(parents=True, exist_ok=True)
    forbidden = {'wakeit-core', 'wakeit-cli', 'wakeit-gateway'}
    for package in ('wakeit-plugin-sdk', 'wakeit-agent-runtime'):
        leaked = dependency_tree(package) & forbidden
        assert not leaked, f'{package} depends on host internals: {sorted(leaked)}'
    assert 'wakeit-plugin-sdk' not in dependency_tree('wakeit-core'), 'Core must not depend on the SDK client'
    print('SDK and official Agents have no Core/CLI/Gateway dependencies; Core does not depend on the SDK.')


if __name__ == '__main__':
    main()
