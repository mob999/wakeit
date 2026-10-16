#!/usr/bin/env python3
"""Exercise the published baseline and workspace migration in a disposable PostgreSQL cluster."""
import getpass
import json
import os
import pathlib
import socket
import subprocess
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]


def main():
    pg = pathlib.Path(subprocess.check_output(['pg_config', '--bindir'], text=True).strip())
    root = ROOT / '.local' / ('workspace-migration-' + uuid.uuid4().hex[:8])
    root.mkdir(parents=True)
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]

    def command(*args, check=True):
        return subprocess.run([str(a) for a in args], text=True, capture_output=True, check=check)

    command(pg / 'initdb', '-D', root / 'pg', '-A', 'trust', '--no-locale', '-E', 'UTF8')
    command(pg / 'pg_ctl', '-D', root / 'pg', '-l', root / 'postgres.log', '-o', f"-p {port} -h 127.0.0.1 -k ''", 'start')
    checks = []
    try:
        url = f'postgres://{getpass.getuser()}@127.0.0.1:{port}/postgres'

        def sql(statement, check=True):
            return command(pg / 'psql', url, '-v', 'ON_ERROR_STOP=1', '-At', '-c', statement, check=check)

        def migrate(name):
            command(pg / 'psql', url, '-v', 'ON_ERROR_STOP=1', '-f', ROOT / 'crates/wakeit/migrations' / name)

        migrate('0001_initial.sql')
        user, runner, run = [str(uuid.uuid4()) for _ in range(3)]
        sql(f"""INSERT INTO users(id,username,password_hash,role) VALUES('{user}','migration-fixture','not-a-login-hash','admin');
        INSERT INTO runners(id,name,token_hash,public_key) VALUES('{runner}','shared-name','fixture-device-hash','fixture-public-key');
        INSERT INTO tasks(id,runner_id,config,enabled,created_by,updated_by) VALUES('same-task','{runner}','{{}}',true,'{user}','{user}');
        INSERT INTO runs(id,task_id,task_revision,runner_id,config,scheduled_at,deadline,source) VALUES('{run}','same-task',1,'{runner}','{{}}',now(),now(),'manual');
        INSERT INTO events(run_id,seq,kind,data) VALUES('{run}',1,'log','{{"text":"synthetic"}}');
        INSERT INTO credentials(hash,user_id,kind) VALUES('fixture-key','{user}','api'),('fixture-session','{user}','session');""")
        migrate('0002_workspaces.sql')
        default = sql('SELECT id FROM workspaces WHERE is_default').stdout.strip()
        assert sql('SELECT count(*) FROM users WHERE instance_admin').stdout.strip() == '0'
        assert sql(f"SELECT role FROM workspace_members WHERE workspace_id='{default}' AND user_id='{user}'").stdout.strip() == 'admin'
        assert sql("SELECT count(*) FROM credentials WHERE (kind='api' AND workspace_id IS NOT NULL) OR (kind='session' AND workspace_id IS NULL)").stdout.strip() == '2'
        assert sql(f"SELECT token_hash||':'||public_key FROM runners WHERE id='{runner}'").stdout.strip() == 'fixture-device-hash:fixture-public-key'
        assert sql(f"SELECT count(*) FROM events WHERE run_id='{run}' AND workspace_id='{default}'").stdout.strip() == '1'
        checks.append('legacy identities, memberships, credentials, runs and events preserved without instance-admin escalation')

        other, other_runner = str(uuid.uuid4()), str(uuid.uuid4())
        sql(f"""INSERT INTO workspaces(id,name) VALUES('{other}','B');
        INSERT INTO runners(id,workspace_id,name,token_hash,public_key) VALUES('{other_runner}','{other}','shared-name','fixture-b-token','fixture-b-key');
        INSERT INTO tasks(workspace_id,id,runner_id,config,enabled,created_by,updated_by) VALUES('{other}','same-task','{other_runner}','{{}}',true,'{user}','{user}');""")
        assert sql("SELECT count(*) FROM tasks WHERE id='same-task'").stdout.strip() == '2'
        checks.append('same task ID and Runner name accepted in independent workspaces')
        result = sql(f"UPDATE tasks SET runner_id='{runner}' WHERE workspace_id='{other}'", check=False)
        assert result.returncode != 0 and 'foreign key' in result.stderr
        result = sql(f"INSERT INTO events(workspace_id,run_id,seq,kind,data) VALUES('{other}','{run}',2,'log','{{}}')", check=False)
        assert result.returncode != 0 and 'foreign key' in result.stderr
        result = sql(f"INSERT INTO credentials(hash,user_id,kind,workspace_id) VALUES('bad-session','{user}','session','{other}')", check=False)
        assert result.returncode != 0 and 'check constraint' in result.stderr
        checks.append('database rejects cross-workspace references and workspace-bound browser sessions')

        sql('DROP SCHEMA public CASCADE; CREATE SCHEMA public;')
        migrate('0001_initial.sql')
        migrate('0002_workspaces.sql')
        assert sql('SELECT count(*) FROM workspaces WHERE is_default').stdout.strip() == '1'
        assert sql('SELECT count(*) FROM settings').stdout.strip() == '1'
        checks.append('fresh installation with both migrations')
        for item in checks:
            print('PASS ' + item)
        (root / 'summary.json').write_text(json.dumps({'checks': checks}, indent=2))
    finally:
        command(pg / 'pg_ctl', '-D', root / 'pg', 'stop', '-m', 'fast', check=False)


if __name__ == '__main__':
    main()
