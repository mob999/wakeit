"""Upgrade a disposable 0006 service during execution without replacing data or restarting Runners."""
import argparse
import getpass
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import uuid
from integration import call, command, wait


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--stack',type=Path,required=True)
    args=parser.parse_args()
    repository=Path(__file__).resolve().parents[1]
    summary=json.loads(args.stack.read_text());root=Path(summary['root'])
    assert root.parent==repository/'.local' and root.name.startswith('workspaces-'),'Only disposable stacks from this checkout are accepted'
    token=json.loads((root/'test-credentials.json').read_text())['admin_token'];workspace=summary['workspaces'][0]
    def api(path,method='GET',body=None):return call(summary['base_url'],'/api/v1/workspaces/'+workspace+path,method,body,token)
    pg=Path(command(['pg_config','--bindir']).strip());database=f"postgres://{getpass.getuser()}@127.0.0.1:{summary['pg_port']}/postgres"
    def sql(query):return command([pg/'psql',database,'-At','-v','ON_ERROR_STOP=1','-c',query]).strip()
    assert sql('SELECT max(version) FROM _sqlx_migrations')=='6'
    runner=next(r for r in api('/runners') if not r['revoked'])
    key=uuid.uuid4().hex
    node=dict(id='source',name='Upgrade fixture',pre=[],runner=runner['id'],agent='mock-events',cwd=str(root/'project-a'),prompt=json.dumps(dict(retry_fixture=key,fail_first=0,sleep=8,text='Live upgrade result')),timeout='20s')
    c=dict(version=1,id='live-upgrade',name='Live upgrade',schedule=None,execution=dict(timeout='1h',start_grace='30s',overlap='skip',misfire='latest'),dag=dict(nodes=[node,dict(node,id='after',pre=['source'],prompt='{{ nodes.source.result }}')]))
    api('/tasks/'+c['id'],'PUT',dict(config=c,expected_revision=None));id=api('/tasks/'+c['id']+'/runs','POST',{})['id']
    before=wait(lambda:(r if (r:=api('/runs/'+id))['nodes'][0]['state']=='running' else None),'old Gateway claims the real execution')
    execution=before['nodes'][0]['execution_id']
    identity=sql("SELECT md5(string_agg(id::text||token_hash||public_key,',' ORDER BY id)) FROM runners")
    old_pids=summary['processes'][3:]
    new_binary=root/'wakeit-upgraded';shutil.copy2(repository/'target/debug/wakeit',new_binary)
    env=dict(os.environ,HOME=str(root/'home'),XDG_CONFIG_HOME=str(root/'home'),WAKEIT_DATABASE_URL=database)
    for i in [2,1]:
        pid=summary['processes'][i]
        line=subprocess.check_output(['ps','-p',str(pid),'-o','command='],text=True)
        assert str(root) in line and 'gateway start' in line
        os.kill(pid,signal.SIGTERM)
        wait(lambda:subprocess.run(['kill','-0',str(pid)],capture_output=True).returncode!=0,'old Gateway stops')
    command([new_binary,'gateway','migrate','--config',root/'gateway.yaml'],env)
    for i,name in [(1,'gateway'),(2,'gateway-second')]:
        with open(root/(name+'.log'),'ab',buffering=0) as log:
            process=subprocess.Popen([str(new_binary),'gateway','start','--config',str(root/(name+'.yaml'))],cwd=repository,env=env,stdout=log,stderr=log,start_new_session=True)
        summary['processes'][i]=process.pid
    summary['gateway_binary']=str(new_binary);args.stack.write_text(json.dumps(summary,indent=2))
    final=wait(lambda:(r if (r:=api('/runs/'+id))['finished_at'] else None),'same execution finishes after upgrade',timeout=40)
    assert final['state']=='succeeded' and final['nodes'][0]['execution_id']==execution
    assert final['window_started_at']==before['started_at']==final['started_at']
    assert sql("SELECT md5(string_agg(id::text||token_hash||public_key,',' ORDER BY id)) FROM runners")==identity
    assert all(subprocess.run(['kill','-0',str(pid)],capture_output=True).returncode==0 for pid in old_pids)
    assert (root/'project-a'/('.retry-'+key)).read_text()=='1'
    assert final['config']['execution']['timeout']=='1h'
    check='Live 0006-to-0007 upgrade preserves the active Run/claim, first-start clock, Runner processes and device credentials without repeating execution'
    summary['checks'].append(check);args.stack.write_text(json.dumps(summary,indent=2));print('PASS '+check)

if __name__=='__main__':main()
