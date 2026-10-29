#!/usr/bin/env python3
"""Manage only a named disposable workspace preview; never touches installed Wakeit services."""
import argparse
import getpass
import json
import os
import pathlib
import shlex
import shutil
import signal
import subprocess
import time
from integration import ROOT, BIN, GATEWAY, call, command, wait


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=['status', 'start', 'stop', 'upgrade'])
    parser.add_argument('--state', type=pathlib.Path, required=True)
    parser.add_argument('--github-env', type=pathlib.Path)
    args = parser.parse_args()
    state = args.state.resolve()
    summary = json.loads(state.read_text())
    root = pathlib.Path(summary['root']).resolve()
    assert root.parent == (ROOT / '.local').resolve() and root.name.startswith('workspaces-') and state.parent == root, 'not a workspace acceptance preview'
    pg = pathlib.Path(command(['pg_config', '--bindir']).strip())
    env = {k: v for k, v in os.environ.items() if not k.startswith('WAKEIT_')}
    env.update(HOME=str(root / 'home'), XDG_CONFIG_HOME=str(root / 'home'), WAKEIT_DATABASE_URL=f"postgres://{getpass.getuser()}@127.0.0.1:{summary['pg_port']}/postgres")
    secret_file = args.github_env or (pathlib.Path(summary['github_env']) if summary.get('github_env') else None)
    if secret_file:
        for line in secret_file.read_text().splitlines():
            if line.startswith(('WAKEIT_GITHUB_CLIENT_ID=', 'WAKEIT_GITHUB_CLIENT_SECRET=')):
                key, value = line.split('=', 1)
                env[key] = shlex.split(value)[0]
        assert env.get('WAKEIT_GITHUB_CLIENT_ID') and env.get('WAKEIT_GITHUB_CLIENT_SECRET'), 'OAuth credentials are incomplete'

    def owned(pid):
        result = subprocess.run(['ps', '-p', str(pid), '-o', 'command='], text=True, capture_output=True)
        if result.returncode:
            return False
        assert str(root) in result.stdout, f'PID {pid} no longer belongs to this preview'
        return True

    def stop(index):
        pid = summary['processes'][index]
        if owned(pid):
            os.kill(pid, signal.SIGTERM)
            for _ in range(50):
                if not owned(pid):
                    break
                time.sleep(.1)
            else:
                raise RuntimeError(f'preview process {pid} did not stop; no replacement was started')

    def spawn(index, name, arguments):
        with open(root / (name + '.log'), 'ab', buffering=0) as log:
            process = subprocess.Popen([str(a) for a in arguments], cwd=ROOT, env=env, stdout=log, stderr=log, start_new_session=True)
        summary['processes'][index] = process.pid

    if args.action == 'status':
        print(json.dumps({'url': summary['base_url'], 'health': call(summary['base_url'], '/health/ready'), 'processes': [owned(pid) for pid in summary['processes']]}))
        return
    if args.action == 'stop':
        for index in reversed(range(len(summary['processes']))):
            stop(index)
        subprocess.run([str(pg / 'pg_ctl'), '-D', str(root / 'pg'), 'stop', '-m', 'fast'], check=True, capture_output=True)
        print('Preview stopped; database, identity and configuration files retained.')
        return
    if args.action == 'upgrade':
        command([pg / 'pg_dump', env['WAKEIT_DATABASE_URL'], '-Fc', '-f', root / ('backup-' + str(int(time.time())) + '.dump')])
        for index in [1, 2]:
            stop(index)
        candidate = root / 'wakeit-gateway.next'
        shutil.copy2(GATEWAY, candidate)
        candidate.replace(root / 'wakeit-gateway')
        if secret_file:
            for name in ['gateway.yaml', 'gateway-second.yaml']:
                file = root / name
                config = json.loads(file.read_text())
                config['auth'] = {'mode': 'github', 'session_cookie': 'wakeit_workspace_preview', 'demo_workspace': True, 'github': {'client_id_env': 'WAKEIT_GITHUB_CLIENT_ID', 'client_secret_env': 'WAKEIT_GITHUB_CLIENT_SECRET'}}
                file.write_text(json.dumps(config))
            summary['github_env'] = str(secret_file.resolve())
    else:
        status = subprocess.run([str(pg / 'pg_ctl'), '-D', str(root / 'pg'), 'status'], capture_output=True)
        if status.returncode:
            command([pg / 'pg_ctl', '-D', root / 'pg', '-l', root / 'postgres.log', '-o', f"-p {summary['pg_port']} -h 127.0.0.1 -k ''", 'start'])
        if not owned(summary['processes'][0]):
            spawn(0, 'nats', [shutil.which('nats-server'), '--config', root / 'nats/nats.conf'])
    for index, name in [(1, 'gateway'), (2, 'gateway-second')]:
        if args.action == 'upgrade' or not owned(summary['processes'][index]):
            spawn(index, name, [root / 'wakeit-gateway', 'start', '--config', root / (name + '.yaml')])
    for index, label in [(3, 'a'), (4, 'b')]:
        if not owned(summary['processes'][index]):
            binary = root / 'legacy-runner' if label == 'a' and (root / 'legacy-runner').exists() else root / 'wakeit'
            spawn(index, 'runner-' + label, [binary, 'runner', 'start', '--config', root / ('runner-' + label) / 'runner.yaml'])
    state.write_text(json.dumps(summary, indent=2))
    wait(lambda: call(summary['base_url'], '/health/ready'), 'preview Gateway')
    wait(lambda: call(summary['other_url'], '/health/ready'), 'second preview Gateway')
    print(json.dumps({'url': summary['base_url'], 'health': 'ready', 'github': bool(secret_file)}))


if __name__ == '__main__':
    main()
