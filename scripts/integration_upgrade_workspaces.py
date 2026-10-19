#!/usr/bin/env python3
"""Upgrade a synthetic 0.1.1 service with its SQLx ledger and a continuously running legacy Runner."""
import argparse
import getpass
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import uuid
from integration import ROOT, BIN, MOCK, call, command, expect_status, port, wait


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--legacy-binary', type=pathlib.Path, required=True)
    args = parser.parse_args()
    root = ROOT / '.local' / ('workspace-upgrade-' + uuid.uuid4().hex[:8])
    root.mkdir(parents=True)
    (root / 'home').mkdir()
    old, new = root / 'legacy', root / 'candidate'
    shutil.copy2(args.legacy_binary, old)
    shutil.copy2(BIN, new)
    pg = pathlib.Path(command(['pg_config', '--bindir']).strip())
    pg_port, nats_port, ws_port, gateway_port = [port() for _ in range(4)]
    url = f'postgres://{getpass.getuser()}@127.0.0.1:{pg_port}/postgres'
    env = {k: v for k, v in os.environ.items() if not k.startswith('WAKEIT_')}
    env.update(HOME=str(root / 'home'), XDG_CONFIG_HOME=str(root / 'home'), WAKEIT_DATABASE_URL=url, WAKEIT_PASSWORD='workspace-upgrade-fixture-password')
    processes, logs, checks = [], [], []
    started = False

    def spawn(name, args):
        file = open(root / (name + '.log'), 'ab', buffering=0)
        logs.append(file)
        p = subprocess.Popen([str(a) for a in args], cwd=ROOT, env=env, stdout=file, stderr=file, start_new_session=True)
        processes.append(p)
        return p

    def stop(p):
        if p.poll() is None:
            p.terminate()
            try:
                p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                p.kill()
                p.wait()

    def sql(query):
        return command([pg / 'psql', url, '-At', '-v', 'ON_ERROR_STOP=1', '-c', query]).strip()

    def passed(name):
        checks.append(name)
        print('PASS ' + name, flush=True)

    try:
        command([pg / 'initdb', '-D', root / 'pg', '-A', 'trust', '--no-locale', '-E', 'UTF8'])
        command([pg / 'pg_ctl', '-D', root / 'pg', '-l', root / 'postgres.log', '-o', f"-p {pg_port} -h 127.0.0.1 -k ''", 'start'])
        started = True
        auth_dir = root / 'nats'
        command([old, 'gateway', 'nats-init', '--dir', auth_dir, '--port', nats_port, '--ws-port', ws_port], env)
        spawn('nats', [shutil.which('nats-server'), '--config', auth_dir / 'nats.conf'])
        base = f'http://127.0.0.1:{gateway_port}'
        config = dict(version=1, listen=f'127.0.0.1:{gateway_port}', public_url=base, database_url_env='WAKEIT_DATABASE_URL', nats_url=f'nats://127.0.0.1:{nats_port}', nats_public_url=f'ws://127.0.0.1:{ws_port}', nats_auth_dir=str(auth_dir))
        file = root / 'gateway.yaml'
        file.write_text(json.dumps(config))
        command([old, 'gateway', 'migrate', '--config', file], env)
        assert sql('SELECT string_agg(version::text,\',\' ORDER BY version) FROM _sqlx_migrations') == '1', 'legacy binary must contain only the published initial migration'
        checksum = sql("SELECT encode(checksum,'hex') FROM _sqlx_migrations WHERE version=1")
        command([old, 'gateway', 'bootstrap', '--config', file, '--username', 'upgrade-admin'], env)
        gateway = spawn('gateway-old', [old, 'gateway', 'start', '--config', file])
        wait(lambda: call(base, '/health/ready'), 'legacy Gateway')
        session = call(base, '/api/v1/session', 'POST', {'username': 'upgrade-admin', 'password': env['WAKEIT_PASSWORD']})['token']

        def legacy(path, method='GET', body=None):
            return call(base, '/api/v1' + path, method, body, session)

        user = legacy('/session')['id']
        key = legacy('/tokens', 'POST', {'name': 'preserved-key'})['token']
        enrollment = legacy('/runners/enrollment', 'POST', {})['token']
        runner_dir = root / 'runner'
        command([old, '--gateway', base, 'runner', 'enroll', '--name', 'upgrade-runner', '--token-env', 'WAKEIT_FIXTURE_ENROLLMENT', '--dir', runner_dir], dict(env, WAKEIT_FIXTURE_ENROLLMENT=enrollment))
        runner = legacy('/runners')[0]['id']
        cwd = root / 'project'
        cwd.mkdir()
        adapter = runner_dir / 'agents/mock.yaml'
        adapter.write_text(json.dumps(dict(version=1, id='mock-events', name='Mock Events', mode='sync', submit=dict(command=str(MOCK), args=['events'], stdin='{{task.prompt}}'), detect=dict(command=str(MOCK), args=['--version']), output=dict(format='events'))))
        runner_file = runner_dir / 'runner.yaml'
        runner_config = dict(version=1, gateway=base, id=runner, name='upgrade-runner', token_file=str(runner_dir / 'device.token'), nats_url=config['nats_public_url'], credentials_file=str(runner_dir / 'runner.creds'), data_dir=str(runner_dir / 'data'), agents=[str(adapter)], allowed_roots=[str(cwd)])
        runner_file.write_text(json.dumps(runner_config))
        preserved_files = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in [runner_file, runner_dir / 'device.token', runner_dir / 'runner.creds']}
        device = spawn('runner-old', [old, 'runner', 'start', '--config', runner_file])
        wait(lambda: any(r['online'] and r['agents'] for r in legacy('/runners')), 'legacy Runner')
        task = dict(version=1, id='preserved-task', name='Preserved task', runner='upgrade-runner', agent='mock-events', cwd=str(cwd), prompt=json.dumps({'text': '# Preserved report', 'count': 3}), schedule=None, enabled=True)
        saved = legacy('/tasks/preserved-task', 'PUT', {'config': task, 'expected_revision': None})
        historic = legacy('/tasks/preserved-task/runs', 'POST', {})['id']
        wait(lambda: legacy('/runs/' + historic)['state'] == 'succeeded', 'historical result')
        delayed = dict(task, prompt=json.dumps({'text': '# Buffered report', 'count': 10, 'sleep': 2}))
        saved = legacy('/tasks/preserved-task', 'PUT', {'config': delayed, 'expected_revision': saved['revision']})
        buffered = legacy('/tasks/preserved-task/runs', 'POST', {})['id']
        wait(lambda: legacy('/runs/' + buffered)['state'] in ['starting', 'running'], 'execution before upgrade')
        stop(gateway)
        command([pg / 'pg_dump', url, '-Fc', '-f', root / 'before-upgrade.dump'])
        command([new, 'gateway', 'migrate', '--config', file], env)
        assert sql("SELECT encode(checksum,'hex') FROM _sqlx_migrations WHERE version=1") == checksum
        assert sql('SELECT count(*) FROM _sqlx_migrations WHERE success') == '2'
        spawn('gateway-new', [new, 'gateway', 'start', '--config', file])
        wait(lambda: call(base, '/health/ready'), 'upgraded Gateway')
        identity = call(base, '/api/v1/session', token=session)
        assert identity['id'] == user and not identity['instance_admin']
        workspace = call(base, '/api/v1/workspaces', token=session)['items'][0]
        assert workspace['role'] == 'admin'

        def api(path, method='GET', body=None, token=key):
            return call(base, '/api/v1/workspaces/' + workspace['id'] + path, method, body, token)

        assert call(base, '/api/v1/session', token=key)['workspace_id'] == workspace['id']
        expect_status(lambda: call(base, '/api/v1/instance/users', token=session), 403)
        assert api('/tasks/preserved-task')['revision'] == saved['revision']
        assert api('/tasks/preserved-task')['config'] == saved['config']
        assert api('/runs/' + historic)['result'] == '# Preserved report'
        result = wait(lambda: (r if (r := api('/runs/' + buffered))['state'] == 'succeeded' else None), 'legacy event replay after migration')
        assert result['result'] == '# Buffered report'
        assert api('/runs/' + buffered + '/events')
        assert device.poll() is None
        assert all(hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest() == digest for path, digest in preserved_files.items())
        passed('SQLx baseline checksum, global sessions, scoped API Key, Task revisions, historical results and original Runner identity survive migration')
        run = api('/tasks/preserved-task/runs', 'POST', {})['id']
        wait(lambda: api('/runs/' + run)['state'] == 'succeeded', 'old Runner executes new workspace assignment')
        passed('continuously running 0.1.1 Runner reconnects, replays buffered output and executes scoped assignments')
        command([new, 'gateway', 'grant-instance-admin', '--config', file, '--user-id', user], env)
        assert call(base, '/api/v1/session', token=session)['instance_admin']
        assert call(base, '/api/v1/instance/users', token=session)
        passed('instance administration requires explicit designation after upgrade')
        (root / 'summary.json').write_text(json.dumps({'checks': checks, 'root': str(root)}, indent=2))
    finally:
        for p in reversed(processes):
            stop(p)
        if started:
            subprocess.run([str(pg / 'pg_ctl'), '-D', str(root / 'pg'), 'stop', '-m', 'fast'], capture_output=True)
        for file in logs:
            file.close()


if __name__ == '__main__':
    main()
