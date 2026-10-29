#!/usr/bin/env python3
"""Select verification jobs from the actual Git diff and validate their results."""
import argparse
import json
import os
from pathlib import Path
import subprocess

SCOPES = ('frontend', 'cli', 'gateway', 'core', 'integration', 'docs', 'packaging', 'installer')
JOBS = ('web', 'cli', 'gateway', 'core', 'integration', 'docs', 'packaging', 'installer')
BACKEND = {'cli', 'gateway', 'core', 'integration'}
PACKAGING_FILES = {
    'scripts/package-release.py', 'scripts/test-release.py', 'scripts/changelog.py',
}


def file_scopes(path):
    if path.startswith(('.github/workflows/', '.github/actions/')) or path in {
        'scripts/ci_scope.py', 'scripts/test-ci-scope.py',
    }:
        return set(SCOPES)
    if path.endswith('.md') or path.startswith(('docs/', '.agents/')) or path in {
        'LICENSE', '.gitignore', '.gitattributes',
    }:
        return {'docs', 'packaging'} if path.startswith('skills/') else {'docs'}
    if path.startswith('web/'):
        return {'frontend', 'packaging'} if path in {
            'web/package.json', 'web/package-lock.json',
        } else {'frontend'}
    if path in {'install.sh', 'deploy-gateway.sh', 'scripts/test-installer.py'}:
        return {'installer', 'packaging'}
    if path in PACKAGING_FILES:
        return {'packaging', 'docs'}
    if path == 'scripts/check-brand-docs.py':
        return {'docs'}
    if path == 'scripts/check-native.py':
        return {'cli', 'gateway'}
    if path.startswith('scripts/'):
        return {'integration'}
    if path.startswith('schemas/'):
        return BACKEND | {'frontend', 'docs', 'packaging'}
    if path.startswith('skills/wakeit/assets/'):
        return BACKEND | {'docs', 'packaging'}
    if path.startswith('skills/'):
        return {'docs', 'packaging'}
    if path in {'Cargo.toml', 'Cargo.lock'} or path.startswith(('rust-toolchain', '.cargo/')):
        return BACKEND | {'packaging'}
    for product in ('cli', 'gateway', 'core'):
        if path.startswith(f'crates/wakeit-{product}/tests/'):
            return {product}
    if path.startswith('crates/wakeit-cli/'):
        return {'cli', 'integration', 'packaging'} if path.endswith('Cargo.toml') else {'cli', 'integration'}
    if path.startswith('crates/wakeit-gateway/'):
        return {'gateway', 'integration', 'packaging'} if path.endswith('Cargo.toml') else {'gateway', 'integration'}
    if path.startswith('agents/'):
        return {'cli', 'integration'}
    if path.startswith('fixtures/'):
        return {'gateway', 'integration'}
    if path.startswith('crates/'):
        return BACKEND.copy()
    if path.startswith(('examples/', 'deploy/')) or path in {
        'Dockerfile', '.dockerignore', 'compose.yaml', '.env.example',
    }:
        return BACKEND | {'docs'}
    # New code/config paths get coverage until their owner is added above.
    return set(SCOPES)


def select_scopes(paths):
    selected = set(SCOPES) if paths is None else set().union(*(file_scopes(p) for p in paths))
    result = {scope: scope in selected for scope in SCOPES}
    result['web'] = bool(selected & {'frontend', 'gateway', 'integration'})
    return result


def changed_paths(event_name, event, cwd=None):
    if event_name == 'pull_request':
        pr = event['pull_request']
        revisions = [f"{pr['base']['sha']}...{pr['head']['sha']}"]
    elif event_name == 'push':
        before = event.get('before', '')
        if not before or set(before) == {'0'}:
            return None
        revisions = [before, event['after']]
    else:
        return None
    output = subprocess.check_output(
        ['git', 'diff', '--name-only', '--no-renames', '-z', *revisions, '--'], cwd=cwd,
    )
    return [p.decode('utf-8') for p in output.split(b'\0') if p]


def check_results(needs):
    if needs.get('scope', {}).get('result') != 'success':
        raise ValueError('Change-scope detection did not succeed')
    scopes = needs['scope']['outputs']
    for job in JOBS:
        selected = scopes.get(job)
        if selected not in ('true', 'false'):
            raise ValueError(f'Missing scope decision for {job}')
        result = needs.get(job, {}).get('result')
        if selected == 'true' and result != 'success':
            raise ValueError(f'Required {job} check ended with {result or "no result"}')
        if selected == 'false' and result not in ('success', 'skipped'):
            raise ValueError(f'{job} check ended with {result or "no result"}')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--check-results', action='store_true')
    args = parser.parse_args()
    if args.check_results:
        check_results(json.loads(os.environ['NEEDS_JSON']))
        print('All selected checks passed.')
        return
    event = json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text())
    scopes = select_scopes(changed_paths(os.environ['GITHUB_EVENT_NAME'], event))
    with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
        for name, enabled in scopes.items():
            output.write(f'{name}={str(enabled).lower()}\n')
    print('Selected checks:', ', '.join(name for name, enabled in scopes.items() if enabled) or 'none')


if __name__ == '__main__':
    main()
