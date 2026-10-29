"""Failure races and process recovery for the same-round node retry path."""
import concurrent.futures
import getpass
import json
import os
import signal
import subprocess
import time
import uuid
from integration import APIError, command, expect_status, wait


def verify(api, a, check, context):
    sql=context['sql'];root=context['root'];binary=context['gateway_binary']
    def config(name, sleep=.1):
        key=uuid.uuid4().hex
        first=dict(id='first',name='First',pre=[],runner=a['id'],agent='mock-events',cwd=a['cwd'],prompt='Retained output',parameters={},timeout='10s')
        source=dict(first,id='source',name='Source',pre=['first'],prompt=json.dumps(dict(retry_fixture=key,fail_first=1,sleep=sleep,text='Recovered output')))
        after=dict(first,id='after',name='After',pre=['source'],prompt='{{ nodes.source.result }}')
        return dict(version=1,id=name,name=name,dag=dict(nodes=[first,source,after]),schedule=None,execution=dict(timeout=None,start_grace='30s',overlap='skip',misfire='latest')),key
    def start(c):
        api('/tasks/'+c['id'],'PUT',dict(config=c,expected_revision=None))
        return api('/tasks/'+c['id']+'/runs','POST',{})['id']
    def end(id):return wait(lambda:(r if (r:=api('/runs/'+id))['finished_at'] else None),'lifecycle graph settles',timeout=40)
    def detail(id):return api('/runs/'+id+'/nodes/source')
    def retry(id):return api('/runs/'+id+'/nodes/source/retry','POST',dict(execution_id=detail(id)['node']['execution_id']))

    c,key=config('manual-gateway-restart',sleep=2)
    id=start(c);initial=end(id);success=initial['nodes'][0]['execution_id']
    result=retry(id)
    wait(lambda:(detail(id)['execution'] or {}).get('state')=='running','manual attempt is claimed')
    summary=json.loads((root/'summary.json').read_text())
    pid=summary['processes'][1]
    commandline=subprocess.check_output(['ps','-p',str(pid),'-o','command='],text=True)
    assert str(root) in commandline and 'wakeit-gateway start' in commandline
    os.kill(pid,signal.SIGTERM)
    wait(lambda:subprocess.run(['kill','-0',str(pid)],capture_output=True).returncode!=0,'test Gateway stops')
    env=dict(os.environ,HOME=str(root/'home'),XDG_CONFIG_HOME=str(root/'home'),WAKEIT_DATABASE_URL=f"postgres://{getpass.getuser()}@127.0.0.1:{summary['pg_port']}/postgres")
    with open(root/'gateway.log','ab',buffering=0) as log:
        process=subprocess.Popen([str(binary),'start','--config',str(root/'gateway.yaml')],cwd=context['repository'],env=env,stdout=log,stderr=log,start_new_session=True)
    summary['processes'][1]=process.pid
    (root/'summary.json').write_text(json.dumps(summary,indent=2))
    final=end(id)
    assert final['state']=='succeeded' and final['nodes'][0]['execution_id']==success
    assert detail(id)['execution']['id']==result['id'] and len(detail(id)['attempts'])==2
    assert (root/'project-a'/('.retry-'+key)).read_text()=='2'
    assert sql("SELECT count(*) FROM outbox WHERE message_id='start:"+result['id']+"'")=='1'
    check('Gateway restart preserves the claimed manual attempt and outbox identity without repeating Agent calls')

    c,key=config('manual-cancel-race',sleep=1)
    # Keep the graph active after source failure so cancellation and retry compete under the graph lock.
    parallel=dict(c['dag']['nodes'][0],id='parallel',pre=[],prompt=json.dumps(dict(sleep=5,text='Parallel')))
    c['dag']['nodes'].append(parallel)
    id=start(c)
    wait(lambda:(detail(id)['execution'] or {}).get('state')=='failed','source fails before cancel race')
    expected=detail(id)['node']['execution_id']
    def click(action):
        try:
            return api('/runs/'+id+('/cancel' if action=='cancel' else '/nodes/source/retry'),'POST',{} if action=='cancel' else dict(execution_id=expected),gateway=context['other'] if action=='cancel' else context['base'])
        except APIError as e:
            assert e.status==409;return None
    with concurrent.futures.ThreadPoolExecutor(2) as pool:list(pool.map(click,['cancel','retry']))
    final=end(id);assert final['state']=='cancelled'
    assert sql("SELECT count(*) FROM runs WHERE parent_run_id='"+id+"' AND finished_at IS NULL")=='0'
    assert len(detail(id)['attempts'])<=2
    check('Concurrent cancellation/retry leaves no active child or duplicate successor')

    c,key=config('manual-runner-identity')
    original=next(r['name'] for r in api('/runners') if r['id']==a['id'])
    for n in c['dag']['nodes']:n['runner']=original
    id=start(c);assert end(id)['state']=='failed'
    try:
        renamed=original+'-renamed'
        sql("UPDATE runners SET name='"+renamed+"' WHERE id='"+a['id']+"'")
        retry(id);assert end(id)['state']=='succeeded'
    finally:sql("UPDATE runners SET name='"+original+"' WHERE id='"+a['id']+"'")
    check('Manual retry retains the frozen Runner UUID when its original display name changes')

    # Whole-round CLI retry remains a distinct action using current Task definition.
    current=api('/tasks/'+c['id']);current['config']['dag']['nodes'][1]['prompt']='New whole-round instruction'
    api('/tasks/'+c['id'],'PUT',dict(config=current['config'],expected_revision=current['revision']))
    reply=json.loads(command([binary,'--workspace',a['workspace'],'--json','run','retry',id],context['cli_env']))
    assert reply['id']!=id and end(reply['id'])['state']=='succeeded'
    fresh=api('/runs/'+reply['id']+'/nodes/source')['execution']
    assert fresh['config']['prompt']=='New whole-round instruction'
    assert api('/runs/'+reply['id'])['nodes'][0]['execution_id']!=api('/runs/'+id)['nodes'][0]['execution_id']
    check('CLI retry without --node still starts a new whole graph using current Task configuration')

if __name__=='__main__':
    import argparse
    from pathlib import Path
    from integration import call
    parser=argparse.ArgumentParser(description='Supplemental lifecycle checks on a retained disposable workspace stack')
    parser.add_argument('--stack',type=Path,required=True)
    args=parser.parse_args()
    repository=Path(__file__).resolve().parents[1]
    summary=json.loads(args.stack.read_text());root=Path(summary['root'])
    assert root.parent==repository/'.local' and root.name.startswith('workspaces-'),'Use only a disposable stack from this checkout'
    token=json.loads((root/'test-credentials.json').read_text())['admin_token']
    workspace=summary['workspaces'][0]
    def api(path,method='GET',body=None,gateway=summary['base_url'],**kwargs):
        return call(gateway,'/api/v1/workspaces/'+workspace+path,method,body,token,**kwargs)
    runner=next(r for r in api('/runners') if not r['revoked'])
    a=dict(id=runner['id'],workspace=workspace,cwd=str(root/'project-a'))
    pg=Path(command(['pg_config','--bindir']).strip())
    database=f"postgres://{getpass.getuser()}@127.0.0.1:{summary['pg_port']}/postgres"
    def sql(query):return command([pg/'psql',database,'-At','-v','ON_ERROR_STOP=1','-c',query]).strip()
    checks=[]
    def check(name):checks.append(name);print('PASS '+name,flush=True)
    cli_env=dict(os.environ,HOME=str(root/'home'),XDG_CONFIG_HOME=str(root/'home'),WAKEIT_TOKEN=token,WAKEIT_GATEWAY=summary['base_url'])
    verify(api,a,check,dict(sql=sql,root=root,binary=root/'wakeit',repository=repository,base=summary['base_url'],other=summary['other_url'],cli_env=cli_env))
    summary=json.loads(args.stack.read_text())
    summary['checks'].extend(checks)
    args.stack.write_text(json.dumps(summary,indent=2))
