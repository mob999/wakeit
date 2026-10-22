#!/usr/bin/env python3
"""Check pre-session upgrade, data preservation, idempotence and refusal of unknown baselines."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[1]
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pg-bin',type=Path)
    args=parser.parse_args()
    pg=args.pg_bin or Path(subprocess.check_output(['pg_config','--bindir'],text=True).strip())
    with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
    (ROOT/'.local').mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='session-upgrade-',dir=ROOT/'.local') as directory:
        folder=Path(directory)
        def run(arguments,env=None,input=None,ok=True):
            result=subprocess.run([str(a) for a in arguments],env=env,input=input,text=True,capture_output=True)
            if ok and result.returncode:raise AssertionError(result.stderr)
            return result
        run([pg/'initdb','-D',folder/'pg','-A','trust','--no-locale','-E','UTF8'])
        run([pg/'pg_ctl','-D',folder/'pg','-l',folder/'pg.log','-o',f"-p {port} -h 127.0.0.1 -k ''",'start'])
        try:
            env=dict(os.environ,WAKEIT_DATABASE_URL=f'postgres://127.0.0.1:{port}/postgres',PGDATABASE=f'postgres://127.0.0.1:{port}/postgres')
            def sql(statement):return run([pg/'psql','--dbname',env['WAKEIT_DATABASE_URL'],'-X','-qAt','-v','ON_ERROR_STOP=1'],env,input=statement).stdout.strip()
            fixture=(ROOT/'fixtures/database/pre-device-0001.sql').read_text(encoding='utf-8')
            checksum=hashlib.sha384(fixture.encode()).hexdigest()
            sql(fixture)
            sql(f"CREATE TABLE _sqlx_migrations(version bigint PRIMARY KEY,description text NOT NULL,installed_on timestamptz NOT NULL DEFAULT now(),success boolean NOT NULL,checksum bytea NOT NULL,execution_time bigint NOT NULL); INSERT INTO _sqlx_migrations VALUES(1,'initial',now(),true,decode('{checksum}','hex'),0);")
            sql("""INSERT INTO users(id,username,password_hash,role) VALUES('00000000-0000-0000-0000-000000000001','preserved','hash','admin');
              INSERT INTO credentials(hash,user_id,kind,name) SELECT 'preserved-key',id,'api','preserved-key' FROM users;
              INSERT INTO runners(id,name,token_hash,public_key) VALUES('00000000-0000-0000-0000-000000000002','preserved-device','token-hash','public-key');
              INSERT INTO tasks(id,runner_id,config,enabled,created_by,updated_by) SELECT 'preserved-task',r.id,'{}',false,u.id,u.id FROM runners r CROSS JOIN users u;
              INSERT INTO runs(id,task_id,task_revision,runner_id,config,state,source,scheduled_at,deadline) SELECT '00000000-0000-0000-0000-000000000003',id,revision,runner_id,config,'succeeded','manual',now(),now() FROM tasks;
              INSERT INTO events(run_id,seq,kind,data) SELECT id,1,'log','{"text":"preserved"}' FROM runs;""")
            tables = ['users','credentials','runners','tasks','runs','events','settings']
            # Compare every original column; additive migrations may introduce
            # metadata without altering any legacy value. The legacy singleton
            # settings key is intentionally replaced by workspace ownership.
            columns = {
                table: sql(f"SELECT string_agg(quote_literal(column_name),',' ORDER BY ordinal_position) FROM information_schema.columns WHERE table_schema='public' AND table_name='{table}' AND (table_name<>'settings' OR column_name<>'id')")
                for table in tables
            }
            def snapshot():
                return {
                    table: sql(f"SELECT jsonb_agg((SELECT jsonb_object_agg(key,value) FROM jsonb_each(to_jsonb(t)) WHERE key=ANY(ARRAY[{columns[table]}]))) FROM {table} t")
                    for table in tables
                }
            before=snapshot()
            binary=ROOT/'target/debug/wakeit'
            migrate=[binary,'gateway','migrate','--config',folder/'gateway.yaml']
            (folder/'gateway.yaml').write_text(json.dumps({'version':1,'listen':'127.0.0.1:8080','public_url':'http://127.0.0.1:8080','database_url_env':'WAKEIT_DATABASE_URL','nats_url':'nats://127.0.0.1:4222','nats_public_url':'ws://127.0.0.1:8081','nats_auth_dir':str(folder/'nats')}),encoding='utf-8')
            original=run(migrate,env,ok=False)
            assert original.returncode != 0 and 'previously applied' in original.stderr,original.stderr
            upgrade=['python3',ROOT/'scripts/upgrade-unreleased-device-auth.py','--backup',folder/'backup.dump','--pg-bin',pg]
            run(upgrade,env)
            assert snapshot()==before
            assert sql('SELECT count(*) FROM runs WHERE session IS NOT NULL OR session_seq<>0')=='0'
            assert sql("SELECT count(*) FROM pg_indexes WHERE schemaname='public' AND indexname='runs_session_active'")=='1'
            assert (folder/'backup.dump').stat().st_size>0
            if os.name=='posix':assert (folder/'backup.dump').stat().st_mode & 0o777 == 0o600
            # Legacy repair remains idempotent before later published migrations.
            run(upgrade,env)
            published=hashlib.sha384((ROOT/'crates/wakeit/migrations/0001_initial.sql').read_bytes()).hexdigest()
            assert sql("SELECT encode(checksum,'hex') FROM _sqlx_migrations WHERE version=1")==published
            sql("UPDATE _sqlx_migrations SET checksum=decode('deadbeef','hex') WHERE version=1")
            bad=run(upgrade,env,ok=False)
            assert bad.returncode != 0 and 'Unknown migration baseline' in bad.stderr
            assert snapshot()==before
            sql(f"UPDATE _sqlx_migrations SET checksum=decode('{published}','hex') WHERE version=1")
            run(migrate,env)
            assert sql("SELECT string_agg(version::text,',' ORDER BY version) FROM _sqlx_migrations WHERE success")==','.join(str(int(p.name.split('_')[0])) for p in sorted((ROOT/'crates/wakeit/migrations').glob('*.sql')))
            assert snapshot()==before
            assert sql('SELECT count(*) FROM runs WHERE parent_run_id IS NOT NULL OR node_id IS NOT NULL')=='0'
            assert sql('SELECT count(*) FROM tasks WHERE editor_layout IS NOT NULL')=='0'
            print('Device authorization upgrade: old checksum rejected by normal migrate; backup, data preservation, published baseline plus workspace migration, repeat and unknown-checksum refusal passed.')
        finally:run([pg/'pg_ctl','-D',folder/'pg','-m','immediate','stop'])
if __name__=='__main__':main()
