"""Session checks against the isolated PostgreSQL/NATS/Runner integration stack."""
import concurrent.futures
import datetime
import re
from zoneinfo import ZoneInfo
import json
import os
import sqlite3
import subprocess
import uuid
from pathlib import Path


def configure(runner_dir, remote, mock):
    agent = json.loads((runner_dir/'agents/mock-events.yaml').read_text())
    agent.update(id='mock-session', name='Mock Sessions')
    agent['submit']['args'] = ['session-events','--title','{{session.title}}']
    def operation(mode, with_input=False):
        command = {'command':str(mock),'args':[mode,'{{session.id}}'],'env':{'WAKEIT_MOCK_DIR':str(remote)}}
        if with_input:command['stdin']='{{task.prompt}}'
        return command
    agent['session'] = {'inspect':operation('session-inspect'),'resume':operation('session-resume',True),'interactive':{'command':str(mock),'args':['session-interactive','{{session.id}}',str(remote)]}}
    agent['session']['list']={'command':str(mock),'args':['session-list'],'env':{'WAKEIT_MOCK_DIR':str(remote)}}
    path = runner_dir/'agents/mock-session.yaml'
    path.write_text(json.dumps(agent))
    return path


def verify(api, base, other, sql, task, apply, trigger, final, role_tokens, runner_id, runner_dir, root, check):
    from __main__ import call, expect_status, wait, BIN
    config_file = runner_dir/'runner.yaml'
    project = Path(json.loads(config_file.read_text())['allowed_roots'][0])
    remote = root/'external-jobs'
    stores = remote/'sessions'
    def definition(name, prompt=None, session=None):
        result = task(name,agent='mock-session',prompt=json.dumps(prompt or {'text':'session fixture result'}))
        result['execution']['overlap']='allow'
        if session:result['session']={'id':session}
        return result
    def ready(run):
        return wait(lambda:(r if (r:=api('/runs/'+run)).get('session',{} ) and r['session']['ready'] else None),'saved resumable session')
    def inspect(session, cwd=project, base_url=base, token=None):
        path=f'/api/v1/runners/{runner_id}/agents/mock-session/sessions/inspect'
        body={'id':session,'cwd':str(cwd)}
        return call(base_url,path,'POST',body,token) if token else api(path[len('/api/v1'):],'POST',body)
    def cli(run, config=config_file):
        # This local command must not require Gateway availability or a personal API Key.
        env=dict(os.environ,WAKEIT_TOKEN_FILE=str(root/'nonexistent-key'),WAKEIT_GATEWAY='http://127.0.0.1:1')
        env.pop('WAKEIT_TOKEN',None)
        return subprocess.run([str(BIN),'run','resume',run,'--runner-config',str(config)],env=env,text=True,capture_output=True,timeout=35)
    advertised=next(agent for agent in api('/agents') if agent['id']=='mock-session')
    assert advertised['session_supported']
    apply(definition('session-fresh'))
    first=trigger('session-fresh');final(first);initial=ready(first)
    session=initial['session']['id']
    def check_new_title(row):
        title=row['session']['title']
        assert title and len(title.encode('utf-8'))<=200 and re.search(r'-wakeit-\d{8}-\d{9}$',title),title
        assert row['execution_snapshot']['session_title']==title
        assert 'session_title' not in row['config'] and 'title' not in row['config']
        started=datetime.datetime.fromisoformat(row['started_at'].replace('Z','+00:00')).astimezone(ZoneInfo(api('/settings')['timezone']))
        suffix=started.strftime('%Y%m%d-%H%M%S')+f'{started.microsecond//1000:03d}'
        assert title.endswith('-wakeit-'+suffix),(title,suffix)
        assert row['config']['name'].startswith(title.rsplit('-wakeit-',1)[0])
        return title
    original_title=check_new_title(initial)
    native=initial['session']['command']
    assert native and 'session-interactive' in native and session in native
    assert 'wakeit run resume' not in native and initial['config']['prompt'] not in native
    assert 'WAKEIT_MOCK_DIR' not in native and 'calls.log' not in native
    if os.name!='nt':
        import shlex
        assert native.startswith('cd -- '+shlex.quote(str(project))+' && '),native
        copied=subprocess.run(native,shell=True,text=True,capture_output=True,timeout=20)
        assert copied.returncode==0 and 'continued existing session '+session in copied.stdout,copied.stderr
    assert initial['session']['runner_id']==runner_id and initial['session']['agent']=='mock-session'
    assert Path(initial['session']['cwd']).resolve()==project.resolve()
    inspected=inspect(session)
    assert inspected['id']==session and inspected['title']==original_title
    assert inspect(session,base_url=other,token=role_tokens['operator'])['persisted']
    expect_status(lambda:inspect(session,token=role_tokens['viewer']),403)
    expect_status(lambda:inspect('--last'),400)
    expect_status(lambda:inspect('../session'),400)
    # Missing/wrong-directory responses come from the adapter, with no inference/submission.
    for identity,cwd in [('ses_missing',project),(session,project/'other-cwd')]:
        Path(cwd).mkdir(exist_ok=True)
        try:inspect(identity,cwd)
        except Exception as error:assert getattr(error,'status',None) in [400,502],error
        else:raise AssertionError('invalid session was accepted')
    result=cli(first)
    assert result.returncode==0 and 'continued existing session '+session in result.stdout,result.stderr
    retry=api('/runs/'+first+'/retry','POST',{})['id'];final(retry)
    retry_row=ready(retry)
    assert retry_row['session']['id']!=session and check_new_title(retry_row)!=original_title
    unsupported=task('session-unsupported');unsupported['session']={'id':session}
    expect_status(lambda:apply(unsupported),400)
    check('session discovery, Viewer protection, local offline continuation, explicit binding capability and independent retries')
    assert advertised['session_list_supported']
    def catalog(cwd=project,q=None,cursor=None,base_url=base,token=None,agent='mock-session'):
        path=f'/api/v1/runners/{runner_id}/agents/{agent}/sessions/query'
        body={'cwd':str(cwd),**({'q':q} if q is not None else {}),**({'cursor':cursor} if cursor else {})}
        return call(base_url,path,'POST',body,token) if token else api(path[len('/api/v1'):],'POST',body)
    assert any(item['id']==session for item in catalog()['sessions'])
    expect_status(lambda:catalog(token=role_tokens['viewer']),403)
    expect_status(lambda:catalog(agent='mock-sync'),400)
    expect_status(lambda:catalog(cwd=root),400)
    empty=project/'session-empty-directory';empty.mkdir()
    assert catalog(cwd=empty)['sessions']==[]
    empty.rmdir()
    for index in range(23):
        identity=f'ses_catalog_{index:03}'
        (stores/(identity+'.json')).write_text(json.dumps({'version':1,'id':identity,'cwd':str(project),'persisted':True,'title':f'中文 review {index:02}','updated_at':'2026-10-15T01:00:00Z'}))
    page1=catalog(q='中文')
    assert len(page1['sessions'])==20 and page1['next_cursor']
    page2=catalog(q='中文',cursor=page1['next_cursor'])
    assert len(page2['sessions'])==3 and page2['next_cursor'] is None
    identifiers=[item['id'] for item in page1['sessions']+page2['sessions']]
    assert len(set(identifiers))==23 and identifiers==sorted(identifiers,reverse=True)
    assert len(catalog(q='review',base_url=other,token=role_tokens['operator'])['sessions'])==20
    assert catalog(q='not-in-any-session')['sessions']==[]
    expect_status(lambda:catalog(cursor='invalid-cursor'),400)
    assert not next(agent for agent in api('/agents') if agent['id']=='mock-sync')['session_list_supported']
    check('session list capability, exact-directory discovery, Unicode search, 20-row cursor pages and permissions across Gateways')

    apply(definition('session-bound',session=session))
    bound=trigger('session-bound');final(bound)
    saved_bound=ready(bound)['session']
    assert saved_bound['id']==session and saved_bound['title']==original_title and saved_bound['command'] and 'session-interactive' in saved_bound['command']
    missing=definition('session-missing',session='ses_missing');apply(missing)
    absent=trigger(missing['id']);assert 'session inspection failed' in final(absent,'failed')['reason']
    assert not any(line.endswith(' '+absent) for line in (remote/'calls.log').read_text().splitlines())
    wrong=definition('session-wrong-cwd',session=session);wrong['cwd']=str(project/'other-cwd');apply(wrong)
    rejected=trigger(wrong['id']);assert 'different working directory' in final(rejected,'failed')['reason']
    assert not any(line.endswith(' '+rejected) for line in (remote/'calls.log').read_text().splitlines())
    apply(definition('session-bound-retry',session=session))
    original=trigger('session-bound-retry');final(original)
    again=api('/runs/'+original+'/retry','POST',{})['id'];final(again)
    assert ready(again)['session']['id']==session and ready(again)['session']['title']==original_title
    check('bound sessions resume without submit fallback; missing or mismatched sessions fail before Agent invocation')
    long=definition('session-title-long');long['name']='任务中文'*80;apply(long)
    long_run=trigger(long['id']);final(long_run);check_new_title(ready(long_run))
    literal=definition('session-title-literal');literal['name']='review {{task.prompt}} 中文';apply(literal)
    literal_run=trigger(literal['id']);final(literal_run)
    assert check_new_title(ready(literal_run)).startswith('review {{task.prompt}} 中文-wakeit-')
    assert all(set(item)<= {'id','title','cwd','updated_at'} for item in catalog()['sessions'])
    check('fresh native titles use task name and actual workspace-local millisecond start, retries get new titles, bound titles stay unchanged and UTF-8 names remain bounded')

    for name in ['session-lock-a','session-lock-b']:apply(definition(name,{'sleep':3,'text':'serialized'},session))
    with concurrent.futures.ThreadPoolExecutor() as pool:
        runs=list(pool.map(trigger,['session-lock-a','session-lock-b']))
    results=[wait(lambda run=run:(row if (row:=api('/runs/'+run))['state'] in ['succeeded','skipped'] else None),'cross-task session exclusion') for run in runs]
    assert sorted(row['state'] for row in results)==['skipped','succeeded']
    assert next(row for row in results if row['state']=='skipped')['reason']=='会话正在使用'
    blocked=next(row for row in results if row['state']=='skipped')['id']
    assert not any(line.endswith(' '+blocked) for line in (remote/'calls.log').read_text().splitlines())
    # Synthetic unverified owner exercises cross-Gateway claim protection; cleanup is explicit.
    unknown=str(uuid.uuid4())
    sql(f"INSERT INTO runs(id,task_id,task_revision,runner_id,config,state,source,scheduled_at,deadline) SELECT '{unknown}',id,revision,runner_id,config,'unknown','manual',now(),now()+interval '1 hour' FROM tasks WHERE id='session-lock-a'")
    try:
        blocked=trigger('session-lock-b');assert final(blocked,'skipped')['reason']=='会话正在使用'
        expect_status(lambda:api('/runs/'+unknown+'/retry','POST',{}),409)
    finally:sql(f"UPDATE runs SET state='interrupted',finished_at=now(),reason='fixture verified' WHERE id='{unknown}'")
    # Local journal independently blocks interactive continuation until the uncertain owner is resolved.
    database=runner_dir/'data/journal.sqlite'
    with sqlite3.connect(database) as connection:
        connection.row_factory=sqlite3.Row
        original=dict(connection.execute('SELECT * FROM local_runs WHERE id=?',(first,)).fetchone())
        original.update(id=unknown,phase='unknown',seq=0)
        assignment=json.loads(original['assignment']);assignment['run_id']=unknown;original['assignment']=json.dumps(assignment)
        columns=list(original)
        connection.execute('INSERT INTO local_runs ('+','.join(columns)+') VALUES ('+','.join('?' for _ in columns)+')',[original[column] for column in columns])
    try:
        result=cli(first);assert result.returncode!=0 and 'unverified run' in result.stderr,result.stderr
    finally:
        with sqlite3.connect(database) as connection:connection.execute('DELETE FROM local_runs WHERE id=?',(unknown,))
    assert cli(first).returncode==0
    check('session mutex spans Tasks despite overlap allow; unknown owners block claims and local continuation until verified')

    verify_protocol_rejections(api,task,apply,trigger,final,wait,session,remote,runner_dir,check)

    # session metadata must survive both Runner and Gateway content budgets.
    apply(definition('session-budget',{'text':'metadata survives truncation','sleep':2,'log_bytes':21*1024*1024,'session_after_logs':True}))
    budget=trigger('session-budget')
    wait(lambda:api('/runs/'+budget)['state']=='running','budget fixture starts')
    sql(f"UPDATE runs SET log_bytes=20971519 WHERE id='{budget}'")
    row=final(budget,timeout=60);saved=ready(budget)
    assert row['log_truncated'] and saved['session']['ready']
    assert any(event['kind']=='session' for event in api('/runs/'+budget+'/events?after=0'))
    assert cli(budget).returncode==0
    # Deleted native state must never be replaced by an interactive create.
    marker=stores/(session+'.json');metadata=marker.read_bytes();marker.unlink()
    try:
        result=cli(first);assert result.returncode!=0 and not marker.exists()
    finally:marker.write_bytes(metadata)
    config=json.loads(config_file.read_text());config['id']=str(uuid.uuid4())
    impostor=root/'impostor-runner.json';impostor.write_text(json.dumps(config))
    assert cli(first,impostor).returncode!=0
    (project/'other-cwd').rmdir()
    check('session metadata survives content truncation; deleted sessions and mismatched local Runner identities never create a replacement')


def verify_protocol_rejections(api, task, apply, trigger, final, wait, session, remote, runner_dir, check):
    """An adapter cannot silently replace or omit the bound session confirmation."""
    for name,options,reason in [
        ('session-protocol-omitted',{'omit_session':True},'did not report'),
        ('session-protocol-mismatch',{'reported_session_id':'ses_unexpected'},'different session ID'),
    ]:
        definition=task(name,agent='mock-session',prompt=json.dumps(dict(options,text='protocol response')))
        definition['session']={'id':session};definition['execution']['overlap']='allow';apply(definition)
        run=trigger(name);row=final(run,'unknown')
        assert row['session']['id']==session and not row['session']['ready']
        assert reason in row['reason'],row['reason']
        assert (remote/'calls.log').read_text().splitlines().count('session-resume '+run)==1
        try:api('/runs/'+run+'/retry','POST',{})
        except Exception as error:assert getattr(error,'status',None)==409,error
        else:raise AssertionError('unknown session run was retried')
        blocked=dict(definition,id=name+'-blocked',name=name+' blocked',prompt='blocked')
        apply(blocked);skipped=trigger(blocked['id']);assert final(skipped,'skipped')['reason']=='会话正在使用'
        api('/runs/'+run+'/resolve','POST',{'state':'interrupted','reason':'fixture verified native process has ended'})
        def local_resolved():
            with sqlite3.connect(runner_dir/'data/journal.sqlite') as connection:
                row=connection.execute('SELECT phase FROM local_runs WHERE id=?',(run,)).fetchone()
                return row and row[0]=='done'
        wait(local_resolved,'resolved uncertain session ownership releases local journal')
    check('bound session omission and ID replacement become unknown, retain original identity and block automatic retry and concurrent execution')


def verify_recovered_commands(api, sql, runner_dir, remote, restart, check):
    """Only reconstruct metadata from a completed local snapshot; never run the Agent."""
    from __main__ import wait
    row=next(run for run in api('/runs?task=session-fresh') if run['state']=='succeeded' and run.get('session',{}).get('ready'))
    identity=row['id'];session_id=row['session']['id'];command=row['session']['command']
    unchanged={key:row[key] for key in ['id','state','result','reason','created_at','started_at','finished_at','execution_snapshot']}
    calls=(remote/'calls.log').read_bytes()
    database=runner_dir/'data/journal.sqlite'
    with sqlite3.connect(database) as connection:
        before=connection.execute('SELECT phase,session,assignment,agent FROM local_runs WHERE id=?',(identity,)).fetchone()
        assert before and before[0]=='done' and json.loads(before[1])['id']==session_id
        legacy=connection.execute("SELECT id,assignment,agent FROM local_runs WHERE phase='done' AND session IS NULL LIMIT 1").fetchone()
        connection.execute("UPDATE local_runs SET session=json_remove(session,'$.command') WHERE id=?",(identity,))
    sql(f"UPDATE runs SET session=session-'command' WHERE id='{identity}'")
    assert api('/runs/'+identity)['session'].get('command') is None
    restart()
    restored=wait(lambda:(item if (item:=api('/runs/'+identity))['session'].get('command') else None),'native command recovered from the local completed snapshot',timeout=35)
    assert restored['session']['command']==command and restored['session']['id']==session_id and restored['session']['ready']
    assert {key:restored[key] for key in unchanged}==unchanged
    assert (remote/'calls.log').read_bytes()==calls
    with sqlite3.connect(database) as connection:
        local=connection.execute('SELECT phase,session,assignment,agent FROM local_runs WHERE id=?',(identity,)).fetchone()
        assert local[0]==before[0] and local[2:]==before[2:]
        assert json.loads(local[1])['id']==session_id and json.loads(local[1])['command']==command
        if legacy:
            same=connection.execute('SELECT session,assignment,agent FROM local_runs WHERE id=?',(legacy[0],)).fetchone()
            assert same[0] is None and same[1:]==legacy[1:]
    check('Runner recovery backfills only missing native-command metadata without resubmitting or changing identity, final state, result or legacy runs')
