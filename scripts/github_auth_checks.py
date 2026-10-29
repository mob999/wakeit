#!/usr/bin/env python3
"""Exercise OAuth callbacks and demo isolation against a disposable PostgreSQL database."""
import getpass
import os
import pathlib
import subprocess
import uuid
from integration_support import ROOT, command, port


def main():
    pg = pathlib.Path(command(['pg_config', '--bindir']).strip())
    root = ROOT / '.local' / ('github-auth-' + uuid.uuid4().hex[:8])
    root.mkdir(parents=True)
    pg_port = port()
    command([pg / 'initdb', '-D', root / 'pg', '-A', 'trust', '--no-locale', '-E', 'UTF8'])
    command([pg / 'pg_ctl', '-D', root / 'pg', '-l', root / 'postgres.log', '-o', f"-p {pg_port} -h 127.0.0.1 -k ''", 'start'])
    try:
        url = f'postgres://{getpass.getuser()}@127.0.0.1:{pg_port}/postgres'
        for migration in ['0001_initial.sql', '0002_workspaces.sql']:
            command([pg / 'psql', url, '-v', 'ON_ERROR_STOP=1', '-f', ROOT / 'crates/wakeit-gateway/migrations' / migration])
        env = dict(os.environ, WAKEIT_TEST_DATABASE_URL=url, WAKEIT_TEST_GITHUB_CLIENT_ID='fixture-client', WAKEIT_TEST_GITHUB_CLIENT_SECRET='fixture-client-secret')
        output = command(['cargo', 'test', '-p', 'wakeit-gateway', '--lib', 'github_auth::tests::database_', '--', '--ignored', '--test-threads=1'], env)
        print(output)
    finally:
        subprocess.run([str(pg / 'pg_ctl'), '-D', str(root / 'pg'), 'stop', '-m', 'fast'], capture_output=True, check=True)


if __name__ == '__main__':
    main()
