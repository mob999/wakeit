#!/usr/bin/env python3
"""Real two-workspace, two-Gateway acceptance stack; all state and identities are disposable."""
import argparse
import getpass
import json
import os
import pathlib
import shutil
import subprocess
import uuid
from integration import APIError, call, command, expect_status, port, wait

ROOT = pathlib.Path(__file__).resolve().parents[1]
BIN = ROOT / 'target/debug/wakeit'
GATEWAY = ROOT / 'target/debug/wakeit-gateway'
MOCK = ROOT / 'target/debug/examples/mock_agent'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--keep-running', action='store_true')
    parser.add_argument('--prepare-only', action='store_true', help='Retain only the basic disposable stack for live-upgrade checks')
    parser.add_argument('--gateway-binary', type=pathlib.Path, help='Alternate Gateway binary for a disposable live-upgrade check')
    parser.add_argument('--manual-retry-only', action='store_true', help='Run focused same-Run recovery acceptance on a fresh stack')
    parser.add_argument('--templates-only', action='store_true', help='Run creation-time template acceptance on a fresh stack')
    parser.add_argument('--files-only', action='store_true', help='Run result file and image acceptance on a fresh two-Gateway stack')
    parser.add_argument('--gateway-port', type=int)
    parser.add_argument('--old-runner', type=pathlib.Path, help='Read-only legacy Runner binary used for workspace A')
    args = parser.parse_args()
    pg = pathlib.Path(command(['pg_config', '--bindir']).strip())
    root = ROOT / '.local' / ('workspaces-' + uuid.uuid4().hex[:8])
    root.mkdir(parents=True)
    (root / 'home').mkdir()
    binary=root/'wakeit'
    shutil.copy2(BIN,binary)
    gateway_binary=root/'wakeit-gateway'
    shutil.copy2(args.gateway_binary or GATEWAY,gateway_binary)
    legacy=root/'legacy-runner'
    if args.old_runner: shutil.copy2(args.old_runner,legacy)
    pg_port, nats_port, ws_port, gateway_port, second_port = [port() for _ in range(5)]
    gateway_port = args.gateway_port or gateway_port
    database = f'postgres://{getpass.getuser()}@127.0.0.1:{pg_port}/postgres'
    env = dict(os.environ, HOME=str(root / 'home'), XDG_CONFIG_HOME=str(root / 'home'),
               WAKEIT_DATABASE_URL=database, WAKEIT_PASSWORD='workspace-fixture-password',
               RUST_LOG='wakeit=info,async_nats=warn')
    processes, files, checks = [], [], []
    started, success = False, False

    def spawn(name, arguments):
        log = open(root / (name + '.log'), 'ab', buffering=0)
        files.append(log)
        process = subprocess.Popen([str(a) for a in arguments], cwd=ROOT, env=env, stdout=log, stderr=log, start_new_session=True)
        processes.append(process)
        return process

    def check(name):
        checks.append(name)
        print('PASS ' + name, flush=True)

    try:
        command([pg / 'initdb', '-D', root / 'pg', '-A', 'trust', '--no-locale', '-E', 'UTF8'])
        command([pg / 'pg_ctl', '-D', root / 'pg', '-l', root / 'postgres.log', '-o', f"-p {pg_port} -h 127.0.0.1 -k ''", 'start'])
        started = True
        auth_dir = root / 'nats'
        command([gateway_binary, 'nats-init', '--dir', auth_dir, '--port', nats_port, '--ws-port', ws_port], env)
        spawn('nats', [shutil.which('nats-server'), '--config', auth_dir / 'nats.conf'])
        config = dict(version=1, listen=f'127.0.0.1:{gateway_port}', public_url=f'http://127.0.0.1:{gateway_port}',
                      database_url_env='WAKEIT_DATABASE_URL', nats_url=f'nats://127.0.0.1:{nats_port}',
                      nats_public_url=f'ws://127.0.0.1:{ws_port}', nats_auth_dir=str(auth_dir), nats_replicas=1)
        config_file = root / 'gateway.yaml'
        config_file.write_text(json.dumps(config))
        second_file = root / 'gateway-second.yaml'
        second_file.write_text(json.dumps(dict(config, listen=f'127.0.0.1:{second_port}')))
        command([gateway_binary, 'migrate', '--config', config_file], env)
        command([gateway_binary, 'bootstrap', '--config', config_file, '--username', 'admin'], env)
        spawn('gateway', [gateway_binary, 'start', '--config', config_file])
        spawn('gateway-second', [gateway_binary, 'start', '--config', second_file])
        base, other = config['public_url'], f'http://127.0.0.1:{second_port}'
        wait(lambda: call(base, '/health/ready'), 'workspace Gateway ready')
        wait(lambda: call(other, '/health/ready'), 'second workspace Gateway ready')
        global_headers = {'x-wakeit-request': '1'}
        admin = call(base, '/api/v1/session', 'POST', {'username': 'admin', 'password': env['WAKEIT_PASSWORD']})['token']

        def global_api(path, method='GET', body=None, token=admin, gateway=base):
            return call(gateway, '/api/v1' + path, method, body, token, headers=global_headers)

        account = global_api('/session')
        assert account['instance_admin'] and account['role'] == 'viewer'
        workspace_a = global_api('/workspaces')['items'][0]['id']

        def api(path, method='GET', body=None, workspace=workspace_a, token=admin, gateway=base, key=None):
            return call(gateway, '/api/v1/workspaces/' + workspace + path, method, body, token, key=key, headers=global_headers)

        invite = api('/invites', 'POST', {'role': 'viewer'})['token']
        call(base, '/api/v1/invites/accept', 'POST', {'token': invite, 'username': 'other', 'password': env['WAKEIT_PASSWORD']}, headers=global_headers)
        secondary = call(base, '/api/v1/session', 'POST', {'username': 'other', 'password': env['WAKEIT_PASSWORD']})['token']
        workspace_b = global_api('/workspaces', 'POST', {'name': 'B'}, token=secondary)['id']
        expect_status(lambda: api('/tasks', workspace=workspace_b), 404)
        invite = api('/invites', 'POST', {'role': 'viewer'}, workspace=workspace_b, token=secondary)['token']
        global_api('/invites/accept', 'POST', {'token': invite})
        assert global_api('/workspaces', token=admin, gateway=other)['items'][1]['role'] == 'viewer'
        expect_status(lambda: api('/runners/enrollment', 'POST', {}, workspace=workspace_b), 403)
        expect_status(lambda: global_api('/instance/users', token=secondary), 403)
        expect_status(lambda: global_api('/tasks'), 426)
        check('global accounts, existing-account invitations, per-workspace roles, instance separation and legacy API rejection')

        runners = {}
        for label, workspace, token in [('a', workspace_a, admin), ('b', workspace_b, secondary)]:
            directory = root / ('runner-' + label)
            cwd = root / ('project-' + label)
            cwd.mkdir()
            enrollment = api('/runners/enrollment', 'POST', {}, workspace=workspace, token=token)['token']
            command([binary, '--gateway', base, 'runner', 'enroll', '--name', 'same-device', '--token-env', 'WAKEIT_TEST_ENROLLMENT', '--dir', directory], dict(env, WAKEIT_TEST_ENROLLMENT=enrollment))
            runner = api('/runners', workspace=workspace, token=token)[0]['id']
            agent = dict(version=1, id='mock-events', name='Mock Events', mode='sync',
                         submit=dict(command=str(MOCK), args=['events'], stdin='{{task.prompt}}'),
                         detect=dict(command=str(MOCK), args=['--version']), output=dict(format='events'))
            adapter = directory / 'agents/mock-events.yaml'
            adapter.write_text(json.dumps(agent))
            runner_config = dict(version=1, gateway=base, id=runner, name='same-device', token_file=str(directory / 'device.token'),
                                 nats_url=config['nats_public_url'], credentials_file=str(directory / 'runner.creds'),
                                 data_dir=str(directory / 'data'), concurrency=2, agents=[str(adapter)], allowed_roots=[str(cwd)])
            runner_file = directory / 'runner.yaml'
            runner_file.write_text(json.dumps(runner_config))
            spawn('runner-' + label, [legacy if args.old_runner and label=='a' else binary, 'runner', 'start', '--config', runner_file])
            wait(lambda: any(r['online'] and r['agents'] for r in api('/runners', workspace=workspace, token=token)), 'Runner ' + label)
            task = dict(version=1, id='same-task', name='Workspace ' + label, runner='same-device', agent='mock-events',
                        cwd=str(cwd), prompt=json.dumps({'text': '# Report ' + label, 'count': 2}), schedule=None, enabled=True)
            api('/tasks/same-task', 'PUT', {'config': task, 'expected_revision': None}, workspace=workspace, token=token)
            run = api('/tasks/same-task/runs', 'POST', {}, workspace=workspace, token=token)['id']
            final = wait(lambda: (r if (r := api('/runs/' + run, workspace=workspace, token=token))['state'] == 'succeeded' else None), 'workspace execution ' + label)
            assert final['result'] == '# Report ' + label
            runners[label] = dict(id=runner, run=run, workspace=workspace, token=token, cwd=str(cwd))
        check('same Task ID and Runner name execute independently through real NATS and isolated Runners')

        a, b = runners['a'], runners['b']
        for path in ['/runs/' + b['run'], '/runs/' + b['run'] + '/events', '/runs/' + b['run'] + '/stream',
                     '/runners/' + b['id'] + '/directories']:
            expect_status(lambda path=path: api(path), 404)
        for path in ['/runs/' + b['run'] + '/cancel', '/runs/' + b['run'] + '/retry', '/runners/' + b['id'] + '/revoke']:
            expect_status(lambda path=path: api(path, 'POST', {}), 404)
        for path in ['/tasks?page=1', '/runs?page=1']:
            result = api(path)
            assert result['total'] == 1
            assert result['items'][0]['config']['name'] == 'Workspace a'
        assert api('/runs?task=same-task')[0]['id'] == a['run']
        days = api('/moments/days')
        assert days['total_runs'] == 1
        assert api('/moments?date=' + days['days'][0]['date'])['items'][0]['id'] == a['run']
        assert len(api('/agents')) == 1
        key_a = api('/tokens', 'POST', {'name': 'A only'})['token']
        expect_status(lambda: api('/tasks', workspace=workspace_b, token=key_a), 404)
        assert api('/tasks', token=key_a)[0]['config']['name'] == 'Workspace a'
        check('resource IDs, events, streams, device queries, counts, Moments and workspace API Keys cannot cross scope')

        cli_env = dict(env)
        for name in ['WAKEIT_GATEWAY', 'WAKEIT_WORKSPACE', 'WAKEIT_TOKEN', 'WAKEIT_TOKEN_FILE']:
            cli_env.pop(name, None)
        for workspace in [workspace_a, workspace_b]:
            command([binary, '--gateway', base, '--workspace', workspace, 'login', '--username', 'admin'], cli_env)
        connections = json.loads(command([binary, 'connection', 'list'], cli_env))
        assert len(connections['connections']) == 2
        by_workspace = {c['workspace_id']: c['name'] for c in connections['connections']}
        command([binary, 'connection', 'use', by_workspace[workspace_a]], cli_env)
        assert json.loads(command([binary, '--json', 'task', 'get', 'same-task'], cli_env))['config']['name'] == 'Workspace a'
        report = [json.loads(line) for line in command([binary, '--json', 'run', 'watch', a['run'], '--timeout', '10s'], cli_env).splitlines()]
        assert report[-1]['type'] == 'summary' and report[-1]['data']['result'] == '# Report a'
        command([binary, 'connection', 'use', by_workspace[workspace_b]], cli_env)
        assert json.loads(command([binary, '--json', 'task', 'get', 'same-task'], cli_env))['config']['name'] == 'Workspace b'
        check('isolated CLI profiles select the correct workspace, preserve same Task IDs and stream scoped Run Watch')

        expect_status(lambda: global_api('/workspaces/' + workspace_a + '/leave', 'POST', {}), 409)
        user_b = global_api('/session', token=secondary)['id']
        api('/members/' + user_b, 'DELETE')
        expect_status(lambda: api('/tasks', token=secondary, gateway=other), 404)
        assert api('/tasks', workspace=workspace_b, token=secondary, gateway=other)[0]['config']['name'] == 'Workspace b'
        check('last Admin protection and member removal affect only the selected workspace across Gateways')

        def sql(query): return command([pg / 'psql', database, '-At', '-v', 'ON_ERROR_STOP=1', '-c', query]).strip()
        dag_runs, file_run = {}, None
        if not args.prepare_only:
            from workspace_lock_checks import verify as verify_workspace_locks
            verify_workspace_locks(api, a, sql, cli_env, check)
            if args.templates_only:
                from task_template_checks import verify as verify_templates
                verify_templates(api,a,b,check,dict(base=base,other=other,sql=sql,binary=binary,gateway_binary=gateway_binary,cli_env=cli_env,root=root,global_api=global_api))
            elif args.files_only:
                from result_file_checks import verify as verify_files
                file_run = verify_files(api,a,b,check,dict(other=other))
            else:
                from dag_retry_migration_checks import verify as verify_retry_migration
                verify_retry_migration(sql,check)
                if not args.manual_retry_only:
                    from task_template_checks import verify as verify_templates
                    verify_templates(api,a,b,check,dict(base=base,other=other,sql=sql,binary=binary,gateway_binary=gateway_binary,cli_env=cli_env,root=root,global_api=global_api))
                    from dag_checks import verify as verify_dags
                    dag_runs = verify_dags(api, a, b, check, dict(other=other, sql=sql, binary=binary,gateway_binary=gateway_binary, cli_env=cli_env, workspace=workspace_a, root=root, spawn=spawn))
                    from dag_retry_checks import verify as verify_retries
                    verify_retries(api, a, b, check, dict(other=other, sql=sql))
                from dag_manual_retry_checks import verify as verify_manual
                verify_manual(api, a, b, check, dict(base=base, other=other, sql=sql, binary=binary,gateway_binary=gateway_binary, cli_env=cli_env, root=root, spawn=spawn, global_api=global_api))
                if not args.manual_retry_only:
                    from editor_layout_checks import verify as verify_layout
                    verify_layout(api, a, b, check, dict(base=base, other=other, root=root, binary=binary,gateway_binary=gateway_binary, cli_env=cli_env, global_api=global_api))
                    from result_file_checks import verify as verify_files
                    file_run = verify_files(api,a,b,check,dict(other=other))
        summary = dict(base_url=base, other_url=other, root=str(root), checks=checks, workspaces=[workspace_a, workspace_b],
                       processes=[p.pid for p in processes], pg_port=pg_port, dag_runs=dag_runs, file_run=file_run)
        (root / 'summary.json').write_text(json.dumps(summary, indent=2))
        (root / 'test-credentials.json').write_text(json.dumps({'admin_token': admin, 'other_token': secondary}))
        os.chmod(root / 'test-credentials.json', 0o600)
        print(json.dumps({'base_url': base, 'root': str(root), 'checks': len(checks)}), flush=True)
        success = True
    finally:
        if not (success and args.keep_running):
            for process in reversed(processes):
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
            if started:
                subprocess.run([str(pg / 'pg_ctl'), '-D', str(root / 'pg'), 'stop', '-m', 'fast'], capture_output=True)
        for file in files:
            file.close()


if __name__ == '__main__':
    main()
