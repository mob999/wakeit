"""Exercise the real workspace mutation guard against PostgreSQL FK row locks."""
import concurrent.futures
import pathlib
import queue
import subprocess
import threading
import time

from integration_support import command, wait


def verify(api, runner, sql, env, check):
    task = dict(api('/tasks/same-task')['config'], id='archive-fk-lock', name='Archive FK lock probe')
    saved = api('/tasks/archive-fk-lock', 'PUT', dict(config=task, expected_revision=None))
    workspace = runner['workspace']
    psql = pathlib.Path(command(['pg_config', '--bindir']).strip()) / 'psql'
    client = subprocess.Popen(
        [str(psql), env['WAKEIT_DATABASE_URL'], '-XqAt', '-v', 'ON_ERROR_STOP=1'],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        text=True, bufsize=1,
    )
    lines = queue.Queue()
    def read():
        for line in client.stdout:
            lines.put(line.strip())
        lines.put(None)
    reader = threading.Thread(target=read, daemon=True)
    reader.start()
    def send(text):
        client.stdin.write(text + '\n')
        client.stdin.flush()
    def until(prefix):
        deadline = time.monotonic() + 10
        received = []
        while time.monotonic() < deadline:
            try:
                line = lines.get(timeout=max(.01, deadline - time.monotonic()))
            except queue.Empty:
                break
            if line is None:
                break
            received.append(line)
            if line.startswith(prefix):
                return line[len(prefix):]
        raise AssertionError('PostgreSQL lock probe did not reach ' + prefix + ': ' + '\n'.join(received))
    executor = concurrent.futures.ThreadPoolExecutor(max_workers=1)
    try:
        # Commit DDL first: table-level FK setup locks must not influence the row-lock test.
        send("SET lock_timeout='5s'; CREATE TABLE workspace_fk_probe (workspace_id uuid REFERENCES workspaces(id)); BEGIN;")
        send("SELECT id FROM tasks WHERE workspace_id='" + workspace + "' AND id='archive-fk-lock' FOR UPDATE; SELECT 'LOCKED:' || pg_backend_pid();")
        pid = int(until('LOCKED:'))
        pending = executor.submit(api, '/tasks/archive-fk-lock/archive', 'POST', dict(expected_revision=saved['revision']))
        wait(lambda: sql("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE " + str(pid) + "=ANY(pg_blocking_pids(pid)) AND query LIKE 'SELECT to_jsonb(t.*) AS item FROM tasks t%')") == 't',
             'archive holds workspace guard and waits for the execution task row', timeout=10)
        # Workspace mutations must still serialize; only FK key-share checks may coexist.
        try:
            sql("BEGIN; SELECT id FROM workspaces WHERE id='" + workspace + "' FOR NO KEY UPDATE NOWAIT; ROLLBACK")
        except RuntimeError as error:
            assert 'could not obtain lock' in str(error), error
        else:
            raise AssertionError('workspace mutation guard no longer serializes writers')
        send("INSERT INTO workspace_fk_probe VALUES ('" + workspace + "'); SELECT 'FK_WRITTEN'; ROLLBACK;")
        until('FK_WRITTEN')
        result = pending.result(timeout=10)
        assert result['archived_at'] and result['revision'] == saved['revision'] + 1, result
        assert api('/tasks/archive-fk-lock')['archived_at']
        check('workspace guard serializes mutations without deadlocking execution foreign-key writes')
    finally:
        client.stdin.close()
        if client.poll() is None:
            client.terminate()
        try:
            client.wait(timeout=5)
        except subprocess.TimeoutExpired:
            client.kill()
            client.wait()
        reader.join(timeout=5)
        executor.shutdown(wait=True, cancel_futures=True)
        sql('DROP TABLE IF EXISTS workspace_fk_probe')
