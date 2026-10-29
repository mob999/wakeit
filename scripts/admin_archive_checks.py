"""Additional real-service checks, called by integration.py with its isolated stack."""
from integration_support import call, expect_status, wait, APIError, command, BIN
import concurrent.futures
import datetime
import json
import os
from pathlib import Path
import subprocess
import uuid



def verify(api, base, other, sql, task, apply, trigger, final, role_tokens, runner_id, runner_dir, root, check, ws, global_api):
    old = 'wakeit-account-fixture-password'
    new = 'wakeit-account-fixture-new-password'
    def session(name, password=old):
        return call(base,'/api/v1/session','POST',{'username':name,'password':password})['token']
    def create(name,role='operator'):
        person=global_api('/instance/users','POST',{'username':name,'password':old})
        credential=session(name)
        invite=api('/invites','POST',{'role':role})['token']
        call(base,'/api/v1/invites/accept','POST',{'token':invite},credential)
        return person
    for role in ['viewer','operator']:
        expect_status(lambda:call(base,'/api/v1/instance/users','POST',{'username':'denied','password':old,'role':'viewer'},role_tokens[role]),403)
    expect_status(lambda:global_api('/instance/users','POST',{'username':' ','password':old,'role':'viewer'}),400)
    expect_status(lambda:global_api('/instance/users','POST',{'username':'short','password':'short','role':'viewer'}),400)
    expect_status(lambda:api('/members/'+global_api('/session')['id'],'PUT',{'role':'root'}),400)
    person=create('account-fixture')
    expect_status(lambda:create('account-fixture'),409)
    credential=session(person['username'])
    key=ws.call(base,'/tokens','POST',{'name':'reset-me'},credential)['token']
    expect_status(lambda:call(base,'/api/v1/instance/users/'+person['id']+'/password','POST',{'password':new},role_tokens['operator']),403)
    global_api('/instance/users/'+person['id']+'/password','POST',{'password':new})
    for token in [credential,key]:expect_status(lambda:call(other,'/api/v1/session',token=token),401)
    expect_status(lambda:session(person['username']),401)
    credential=session(person['username'],new)
    key=ws.call(base,'/tokens','POST',{'name':'disable-me'},credential)['token']
    global_api('/instance/users/'+person['id'],'PUT',{'active':False})
    api('/members/'+person['id'],'PUT',{'role':'viewer'})
    assert not next(u for u in global_api('/instance/users') if u['id']==person['id'])['active']
    for token in [credential,key]:expect_status(lambda:call(other,'/api/v1/session',token=token),401)
    expect_status(lambda:session(person['username'],new),401)
    global_api('/instance/users/'+person['id'],'PUT',{'active':True})
    for token in [credential,key]:expect_status(lambda:call(other,'/api/v1/session',token=token),401)
    assert call(other,'/api/v1/session',token=session(person['username'],new))['id']==person['id']
    admin=global_api('/session')
    secondary=create('account-admin-fixture','admin'); secondary_token=session(secondary['username'])
    def demote(member):
        try:api('/members/'+member['id'],'PUT',{'role':'operator','active':True});return 200
        except APIError as e:return e.status
    with concurrent.futures.ThreadPoolExecutor() as pool:codes=list(pool.map(demote,[admin,secondary]))
    assert codes.count(200)==1 and all(code in [200,403,409] for code in codes),codes
    surviving=sql("SELECT m.user_id FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.active AND u.active AND m.role='admin'")
    assert surviving in [admin['id'],secondary['id']]
    if surviving==secondary['id']:
        ws.call(base,'/members/'+admin['id'],'PUT',{'role':'admin','active':True},secondary_token)
    else:api('/members/'+secondary['id'],'PUT',{'role':'admin','active':True})
    global_api('/instance/users/'+secondary['id']+'/password','POST',{'password':new})
    expect_status(lambda:call(other,'/api/v1/session',token=secondary_token),401)
    api('/members/'+secondary['id'],'PUT',{'role':'viewer'});global_api('/instance/users/'+secondary['id'],'PUT',{'active':False})
    api('/members/'+person['id'],'PUT',{'role':'viewer'});global_api('/instance/users/'+person['id'],'PUT',{'active':False})
    with concurrent.futures.ThreadPoolExecutor(max_workers=24) as pool:
        results=list(pool.map(lambda _:api('/members/'+person['id'],'PUT',{'role':'viewer'}),range(24)))
    assert len(results)==24 and all(r['ok'] for r in results)
    check('Web account creation, reset and disable revoke all credentials; concurrent last-admin protection')

    c=task('archive-running',prompt=json.dumps({'sleep':1.5,'text':'archived process completed'}))
    created=apply(c);run=trigger(c['id'])
    wait(lambda:api('/runs/'+run)['state'] in ['starting','running'],'archive run starts')
    expect_status(lambda:ws.call(base,'/tasks/'+c['id']+'/archive','POST',{'expected_revision':created['revision']},role_tokens['viewer']),403)
    expect_status(lambda:api('/tasks/'+c['id']+'/archive','POST',{'expected_revision':999}),409)
    archived=ws.call(other,'/tasks/'+c['id']+'/archive','POST',{'expected_revision':created['revision']},role_tokens['operator'])
    assert archived['archived_at'] and not archived['config']['enabled'] and archived['next_at'] is None
    again=api('/tasks/'+c['id']+'/archive','POST',{'expected_revision':created['revision']});assert again['revision']==archived['revision']
    assert not any(t['id']==c['id'] for t in api('/tasks'))
    assert api('/tasks?archived=true&page=1')['items'][0]['id']==c['id']
    expect_status(lambda:apply(c,archived['revision']),400)
    expect_status(lambda:trigger(c['id']),409)
    result=final(run);assert 'archived process completed' in result['result'] and result['task_archived']
    expect_status(lambda:api('/runs/'+run+'/retry','POST',{}),409)
    assert api('/runs?task='+c['id'])[0]['task_archived']
    assert api('/runs/'+run+'/events')
    expect_status(lambda:ws.call(base,'/tasks?archived=true',token=role_tokens['viewer']),403)
    # A task lock decides archive versus queued claims; existing claims and unknown state remain intact.
    queued=apply(task('archive-queued',schedule={'every':'1s','timezone':'UTC'}))
    run_ids={state:str(uuid.uuid4()) for state in ['queued','starting','unknown']}
    claim=str(uuid.uuid4());runtime=sql("SELECT runtime_id FROM runners WHERE id='"+runner_id+"'")
    for state,run_id in run_ids.items():
        sql(f"INSERT INTO runs(workspace_id,id,task_id,task_revision,runner_id,config,state,source,scheduled_at,deadline,claim_id,runtime_id) SELECT workspace_id,'{run_id}',id,revision,runner_id,config,'{state}','manual',now(),now()+interval '1 hour','{claim}','{runtime}' FROM tasks WHERE id='archive-queued'")
    cancelled=api('/tasks/archive-queued/archive','POST',{'expected_revision':queued['revision']})
    assert api('/runs/'+run_ids['queued'])['state']=='cancelled'
    assert api('/runs/'+run_ids['starting'])['state']=='starting'
    assert api('/runs/'+run_ids['unknown'])['state']=='unknown'
    device=(runner_dir/'device.token').read_text().strip()
    def claim_run(state):return call(base,'/api/v1/device/claim','POST',{'run_id':run_ids[state],'runtime_id':runtime,'claim_id':claim,'agent_digest':'fixture'},device)
    assert not claim_run('queued')['execute'] and claim_run('starting')['execute']
    restore=api('/tasks/archive-queued/restore','POST',{'expected_revision':cancelled['revision']})
    assert restore['archived_at'] is None and not restore['config']['enabled'] and restore['next_at'] is None
    assert api('/tasks/archive-queued/restore','POST',{'expected_revision':restore['revision']})['revision']==restore['revision']
    assert not any(t['id']=='archive-queued' for t in api('/tasks?archived=true'))
    # Resolve synthetic owner records so they do not interfere with later retention/overlap checks.
    sql("UPDATE runs SET state='interrupted',finished_at=now(),reason='fixture cleanup' WHERE task_id='archive-queued' AND state IN ('starting','unknown')")
    resumed=dict(restore['config'],enabled=True)
    updated=apply(resumed,restore['revision']);assert updated['next_at'] is not None
    apply(dict(updated['config'],enabled=False),updated['revision'])
    # Concurrent archive and apply serialize; stale writes cannot resurrect a task.
    raced=apply(task('archive-race'))
    def archive_race():return api('/tasks/archive-race/archive','POST',{'expected_revision':raced['revision']})
    def edit_race():
        try:return apply(dict(raced['config'],name='edited archive race'),raced['revision'])
        except APIError as e:assert e.status in [400,409];return None
    with concurrent.futures.ThreadPoolExecutor() as pool:
        a=pool.submit(archive_race);b=pool.submit(edit_race)
        try:a.result()
        except APIError as e:assert e.status==409
        b.result()
    row=api('/tasks/archive-race')
    if row['archived_at'] is None:api('/tasks/archive-race/archive','POST',{'expected_revision':row['revision']})
    for i in range(22):
        row=apply(task(f'archive-page-{i:02}'));api('/tasks/'+row['id']+'/archive','POST',{'expected_revision':row['revision']})
    first=api('/tasks?archived=true&page=1&page_size=20');second=api('/tasks?archived=true&page=2&page_size=20')
    assert len(first['items'])==20 and second['items'] and not {t['id'] for t in first['items']} & {t['id'] for t in second['items']}
    assert sql("SELECT count(*) FROM audit WHERE action='task.archive' AND target='archive-running'")=='1'
    assert sql("SELECT count(*) FROM audit WHERE action='task.restore' AND target='archive-queued'")=='1'
    cli_key=api('/tokens','POST',{'name':'archive-cli-fixture'})['token']
    cli_env=dict(os.environ,WAKEIT_TOKEN=cli_key)
    cli=lambda *args:json.loads(command([BIN,'--gateway',base,'--json','task',*args],cli_env))
    assert any(t['id']=='archive-race' for t in cli('list','--archived'))
    restored=cli('restore','archive-race');assert not restored['config']['enabled'] and restored['archived_at'] is None
    assert cli('archive','archive-race')['archived_at']
    cli_record=next(t for t in api('/tokens') if t['name']=='archive-cli-fixture');api('/tokens/'+cli_record['id'],'DELETE')
    check('archive preserves running work and history, revokes queued work, blocks edits and retries, and restores paused')
