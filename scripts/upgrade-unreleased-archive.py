#!/usr/bin/env python3
"""Explicit, data-preserving upgrade of the pre-archive 0.1.0 migration baseline."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess

OLD_CHECKSUM = '2b14e5b27f6acf1ee80e9875897a22081586e7e2e95d1f7b71c5d3ea48a1d5c12ff7f92ce74c842bbda6d7ebd0b82d01'
ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--backup', required=True, type=Path, help='New pg_dump custom archive path')
    parser.add_argument('--pg-bin', type=Path)
    args = parser.parse_args()
    if not os.environ.get('WAKEIT_DATABASE_URL'):
        parser.error('set WAKEIT_DATABASE_URL')
    env = dict(os.environ, PGDATABASE=os.environ['WAKEIT_DATABASE_URL'])
    def binary(name):
        return str(args.pg_bin / name) if args.pg_bin else shutil.which(name) or name
    def sql(statement):
        result = subprocess.run([binary('psql'), '--dbname', env['WAKEIT_DATABASE_URL'], '-X', '-qAt', '-v', 'ON_ERROR_STOP=1'],
            input=statement, text=True, env=env, capture_output=True)
        if result.returncode:
            raise SystemExit('Upgrade failed; transaction rolled back. ' + result.stderr)
        return result.stdout.strip()
    target = hashlib.sha384((ROOT/'crates/wakeit-gateway/migrations/0001_initial.sql').read_bytes()).hexdigest()
    rows = sql("SELECT version::text||':'||success::text||':'||encode(checksum,'hex') FROM _sqlx_migrations ORDER BY version").splitlines()
    if rows == ['1:true:'+target]:
        if sql("SELECT count(*) FROM information_schema.columns WHERE table_schema='public' AND ((table_name='tasks' AND column_name='archived_at' AND data_type='timestamp with time zone') OR (table_name='runs' AND column_name='session' AND data_type='jsonb') OR (table_name='runs' AND column_name='session_seq' AND data_type='bigint' AND is_nullable='NO'))") != '3' or sql("SELECT count(*) FROM pg_indexes WHERE schemaname='public' AND indexname='runs_session_active'") != '1':
            raise SystemExit('Target baseline has unexpected schema; refusing upgrade')
        print('Archive baseline already installed')
        return
    if rows != ['1:true:'+OLD_CHECKSUM]:
        raise SystemExit('Unknown migration baseline; refusing upgrade')
    if args.backup.exists():
        raise SystemExit('Backup path exists; refusing overwrite')
    args.backup.parent.mkdir(parents=True, exist_ok=True)
    # Reserve a private file before pg_dump writes it.
    fd = os.open(args.backup, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    os.close(fd)
    dump = subprocess.run([binary('pg_dump'), '--dbname', env['WAKEIT_DATABASE_URL'], '-Fc', '--file', str(args.backup)], env=env, text=True, capture_output=True)
    if dump.returncode:
        raise SystemExit('Backup failed; database unchanged. ' + dump.stderr)
    sql(f"""BEGIN;
        SELECT pg_advisory_xact_lock(743274);
        DO $$ BEGIN
          IF (SELECT count(*) FROM _sqlx_migrations)<>1 OR NOT EXISTS (
            SELECT 1 FROM _sqlx_migrations WHERE version=1 AND success AND checksum=decode('{OLD_CHECKSUM}','hex')
          ) THEN RAISE EXCEPTION 'migration baseline changed'; END IF;
        END $$;
        ALTER TABLE tasks ADD COLUMN archived_at timestamptz;
        DROP INDEX tasks_due;
        CREATE INDEX tasks_due ON tasks(next_at) WHERE enabled AND archived_at IS NULL;
        ALTER TABLE runs ADD COLUMN session jsonb, ADD COLUMN session_seq bigint NOT NULL DEFAULT 0;
        CREATE INDEX runs_session_active ON runs(runner_id,(config->>'agent'),(COALESCE(session->>'id',config->'session'->>'id'))) WHERE state IN ('starting','running','unknown');
-- Browser-confirmed CLI grants; raw codes and credentials are never stored here.
CREATE TABLE device_grants (
 device_hash text PRIMARY KEY, user_hash text NOT NULL UNIQUE, intent jsonb NOT NULL,
 state text NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','approved','denied','consumed')),
 user_id uuid REFERENCES users(id), approval_credential text, api_hash text,
 expires_at timestamptz NOT NULL, last_poll timestamptz, poll_interval integer NOT NULL DEFAULT 5,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE device_auth_limits(key text NOT NULL,created_at timestamptz NOT NULL DEFAULT now());
CREATE INDEX device_auth_limits_time ON device_auth_limits(created_at);
CREATE TABLE device_registrations (
 request_id uuid PRIMARY KEY, runner_id uuid NOT NULL UNIQUE REFERENCES runners(id),
 public_key text NOT NULL UNIQUE, user_id uuid NOT NULL REFERENCES users(id)
);

        UPDATE _sqlx_migrations SET checksum=decode('{target}','hex') WHERE version=1;
        COMMIT;""")
    print('Archive baseline installed; all existing rows preserved. Backup:',args.backup)

if __name__ == '__main__':
    main()
