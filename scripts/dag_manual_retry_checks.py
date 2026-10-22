"""Same-round DAG recovery on fresh PostgreSQL, NATS, two Gateways and real mock Runners."""
import concurrent.futures
import json
import time
import uuid
from integration import APIError, call, command, expect_status, wait


def verify(api, a, b, check, context):
    sql = context['sql']
    def node(id, pre=(), fail_first=0, retries=0, sleep=.1, prompt=None):
        return dict(id=id,name=id,pre=list(pre),runner=a['id'],agent='mock-events',cwd=a['cwd'],prompt=prompt or json.dumps(dict(retry_fixture=uuid.uuid4().hex,fail_first=fail_first,sleep=sleep,text='Result '+id)),timeout='10s',max_retries=retries)
    def config(name, nodes, limit=None, results=None):
        dag=dict(nodes=nodes)
        if results is not None: dag['results']=results
        return dict(version=1,id=name,name=name,schedule=None,enabled=True,execution=dict(timeout=limit,start_grace='30s',overlap='skip',misfire='latest'),dag=dag)
    def start(c):
        api('/tasks/'+c['id'],'PUT',dict(config=c,expected_revision=None))
        return api('/tasks/'+c['id']+'/runs','POST',{})['id']
    def ended(id):
        return wait(lambda:(r if (r:=api('/runs/'+id))['finished_at'] else None),'manual recovery settles',timeout=45)
    def detail(id,n): return api('/runs/'+id+'/nodes/'+n)
    def retry(id,n,execution=None,resume=False,**options):
        if execution is None: execution=detail(id,n)['node'].get('execution_id')
        return api('/runs/'+id+'/nodes/'+n+'/retry','POST',dict(execution_id=execution,resume_stopped=resume),**options)
    def children(id): return {n['id']:n for n in api('/runs/'+id)['nodes']}
    def snap(id,n):
        run=detail(id,n)['execution']
        return {k:run[k] for k in ['id','result','started_at','finished_at','session']}

    c=config('manual-same-round',[node('good'),node('source',['good'],fail_first=2),node('after',['source'],prompt='New input {{ nodes.source.result }}'),node('branch')])
    id=start(c);first=ended(id);assert first['state']=='failed'
    success=snap(id,'good');branch=snap(id,'branch');original=detail(id,'source')['execution']['id']
    assert detail(id,'source')['retry']['action']=='retry'
    assert [n['id'] for n in detail(id,'source')['retry']['affected']]==['after']
    expect_status(lambda:retry(id,'good'),409)
    expect_status(lambda:retry(id,'after'),409)
    expect_status(lambda:api('/runs/'+id+'/nodes/source/retry','POST',{}),422)
    expect_status(lambda:api('/runs/'+id+'/nodes/source/retry','POST',dict(execution_id=original),workspace=b['workspace'],token=b['token']),404)
    # Edit live task: this recovery must still use the frozen old definition.
    task=api('/tasks/'+c['id']);updated=json.loads(json.dumps(c));updated['dag']['nodes'][1]['prompt']='Changed live task'
    api('/tasks/'+c['id'],'PUT',dict(config=updated,expected_revision=task['revision']))
    key=str(uuid.uuid4());body=dict(execution_id=original,resume_stopped=False)
    def click(gateway):
        try:return api('/runs/'+id+'/nodes/source/retry','POST',body,key=key,gateway=gateway)
        except APIError as error:return dict(error=error.status)
    with concurrent.futures.ThreadPoolExecutor(2) as pool:
        replies=list(pool.map(click,[context['base'],context['other']]))
    assert replies[0]==replies[1] and replies[0]['parent_run_id']==id,replies
    second=ended(id);assert second['state']=='failed'
    assert len(detail(id,'source')['attempts'])==2
    time.sleep(6)
    assert len(detail(id,'source')['attempts'])==2,'Manual failure must not schedule an automatic retry'
    assert api('/runs/'+id+'/nodes/source/retry','POST',body,key=key)==replies[0]
    expect_status(lambda:retry(id,'source',original),409)
    expect_status(lambda:api('/runs/'+id+'/nodes/source/retry','POST',dict(execution_id=replies[0]['id']),key=key),409)
    # CLI reads latest execution and uses the guarded same-round endpoint.
    cli=json.loads(command([context['binary'],'--workspace',a['workspace'],'--json','run','retry',id,'--node','source'],context['cli_env']))
    assert cli['parent_run_id']==id and cli['node_id']=='source'
    final=ended(id);assert final['state']=='succeeded',final
    assert final['id']==id and final['started_at']==first['started_at'] and final['finished_at']!=first['finished_at']
    assert snap(id,'good')==success and snap(id,'branch')==branch
    assert len(detail(id,'source')['attempts'])==3 and len(detail(id,'after')['attempts'])==1
    after=detail(id,'after')['execution']
    assert after['config']['prompt']=='New input Result source'
    assert after['execution_snapshot']['input_executions']==[dict(node_id='source',execution_id=cli['id'])]
    assert detail(id,'source')['execution']['attempt_origin']=='manual'
    assert detail(id,'source')['execution']['config']['prompt']==c['dag']['nodes'][1]['prompt']
    assert sum(r['id']==id for r in api('/runs'))==1
    assert sql(f"SELECT count(*) FROM runs WHERE parent_run_id='{id}' AND node_id IN ('good','branch')")=='2'
    check('Same Run, frozen definition, idempotent concurrent manual attempts, CLI and retained success identities/results/session/times')

    id=start(config('manual-exhausted',[node('source',fail_first=2,retries=1),node('after',['source'])]))
    assert ended(id)['state']=='failed'
    assert len(detail(id,'source')['attempts'])==2
    retry(id,'source');assert ended(id)['state']=='succeeded'
    assert len(detail(id,'source')['attempts'])==3
    check('Exhausted automatic budget permits a manual attempt without resetting automatic quota')

    id=start(config('manual-warning',[node('good'),node('bad',fail_first=1)],results=['good']))
    assert ended(id)['state']=='warning';good=snap(id,'good')
    retry(id,'bad');assert ended(id)['state']=='succeeded' and snap(id,'good')==good
    check('Warning recovery retains selected successful output and reaggregates from current attempts')

    id=start(config('manual-independent',[node('left',fail_first=1),node('right',fail_first=1),node('join',['left','right'])]))
    assert ended(id)['state']=='failed'
    right=detail(id,'right')['execution']['id']
    retry(id,'left');assert ended(id)['state']=='failed'
    assert detail(id,'right')['execution']['id']==right and detail(id,'join')['execution'] is None
    retry(id,'right');assert ended(id)['state']=='succeeded'
    assert len(detail(id,'join')['attempts'])==1
    check('Independent failed branches recover explicitly and AND fan-in executes once after all predecessors succeed')

    c=config('manual-stopped',[node('first'),node('slow',['first'],sleep=4),node('after',['slow']),node('unrelated',sleep=8)])
    id=start(c)
    wait(lambda:children(id)['slow']['state']=='running','selected node starts')
    api('/runs/'+id+'/cancel','POST',{});assert ended(id)['state']=='cancelled'
    first=snap(id,'first');before=detail(id,'slow')['execution']['id']
    expect_status(lambda:retry(id,'slow'),409)
    assert detail(id,'slow')['retry']['action']=='resume'
    retry(id,'slow',before,True)
    final=ended(id);assert final['state']=='failed',final  # unrelated cancelled branch remains stopped.
    assert children(id)['after']['state']=='succeeded' and children(id)['unrelated']['state']=='cancelled'
    assert snap(id,'first')==first
    assert final['recovery_generation']==1 and final['window_started_at']!=final['started_at']
    check('Explicit stopped recovery confirms old processes stopped, restores only descendants and keeps original start time')

    c=config('manual-stopped-concurrent',[node('first'),node('slow',['first'],sleep=2),node('after',['slow']),node('unrelated',sleep=4)])
    id=start(c);wait(lambda:children(id)['slow']['state']=='running','both branches start')
    api('/runs/'+id+'/cancel','POST',{});assert ended(id)['state']=='cancelled'
    retry(id,'slow',resume=True)
    wait(lambda:children(id)['slow']['state']=='running','first branch resumes')
    unrelated=api('/runs/'+id+'/nodes/unrelated')
    assert unrelated['retry']['action']=='resume',unrelated['retry']
    retry(id,'unrelated',resume=True)
    assert ended(id)['state']=='succeeded'
    assert api('/runs/'+id)['recovery_generation']==1
    check('Explicit recovery of another stopped branch can join the active recovery window after old attempts stop')

    c=config('manual-time-limit',[node('slow',sleep=3),node('after',['slow'])],limit='1s')
    id=start(c);assert ended(id)['state']=='timed_out'
    expect_status(lambda:retry(id,'slow'),409)
    retry(id,'slow',resume=True)
    assert ended(id)['state']=='timed_out' and api('/runs/'+id)['recovery_generation']==1
    check('Explicit total timeout remains frozen; resumed Run receives a fresh bounded window')

    c=config('manual-no-total',[node('slow',sleep=2)])
    del c['execution']['timeout']
    id=start(c);assert ended(id)['state']=='succeeded'
    assert api('/runs/'+id)['config']['execution']['timeout'] is None
    check('Omitted DAG total timeout stays disabled while each Runner assignment retains a concrete node timeout')

    id=start(config('manual-active-window',[node('source',fail_first=1),node('parallel',sleep=5)],limit='20s'))
    wait(lambda:(detail(id,'source')['execution'] or {}).get('state')=='failed','source failed while graph active')
    before=api('/runs/'+id)
    assert before['finished_at'] is None
    retry(id,'source');assert ended(id)['state']=='succeeded'
    after=api('/runs/'+id)
    assert after['window_started_at']==before['window_started_at'] and after['recovery_generation']==0
    check('Retry in an active Run keeps the existing total time window')

    id=start(config('manual-auto-race',[node('source',fail_first=1,retries=1),node('after',['source'])]))
    waiting=wait(lambda:(d if (d:=detail(id,'source'))['node'].get('retry_at') else None),'automatic successor queued')
    failed=waiting['attempts'][0]['id'];pending=waiting['execution']['id']
    reply=retry(id,'source',failed)
    assert reply['id']==pending and ended(id)['state']=='succeeded'
    assert len(detail(id,'source')['attempts'])==2
    check('A stale failure guard can reuse its single pending automatic successor without adding an attempt')

    # Dispatch-before-execution failure: null is a valid guard, never a missing-field default.
    c=config('manual-before-dispatch',[node('root',fail_first=1),node('after',['root'])])
    id=start(c);assert ended(id)['state']=='failed'
    old=detail(id,'root')['execution']['id']
    sql(f"UPDATE dag_run_nodes SET execution_id=NULL,state='failed',reason='Synthetic dispatch failure' WHERE parent_run_id='{id}' AND node_id='root'")
    reply=api('/runs/'+id+'/nodes/root/retry','POST',dict(execution_id=None));assert reply['parent_run_id']==id
    assert ended(id)['state']=='succeeded'
    assert len(detail(id,'root')['attempts'])==2
    check('Failed nodes without an execution support an explicit null execution guard and preserve earlier history')

    c=config('manual-overlap',[node('source',fail_first=1)])
    id=start(c);assert ended(id)['state']=='failed'
    task=api('/tasks/'+c['id']);updated=json.loads(json.dumps(c));updated['dag']['nodes'][0]['prompt']=json.dumps(dict(sleep=3,text='Other active round'))
    api('/tasks/'+c['id'],'PUT',dict(config=updated,expected_revision=task['revision']))
    other=api('/tasks/'+c['id']+'/runs','POST',{})['id']
    expect_status(lambda:retry(id,'source'),409)
    ended(other)
    paused=api('/tasks/'+c['id']);paused['config']['enabled']=False
    api('/tasks/'+c['id'],'PUT',dict(config=paused['config'],expected_revision=paused['revision']))
    retry(id,'source');assert ended(id)['state']=='succeeded'
    check('Frozen overlap policy rejects concurrent rounds; paused Tasks still permit explicit manual recovery')

    # Result retention and archive/overlap guards are checked again in the transaction.
    c=config('manual-guarded',[node('good'),node('bad',['good'],fail_first=10,prompt='{{ nodes.good.result }}')])
    # Use a failing child with a predecessor reference to exercise unavailable retained input.
    c['dag']['nodes'][1]['prompt']=json.dumps(dict(fail=True,text='{{ nodes.good.result }}'))
    id=start(c);assert ended(id)['state']=='failed'
    good=detail(id,'good')['execution']['id']
    sql(f"UPDATE runs SET result=NULL WHERE id='{good}'")
    expect_status(lambda:retry(id,'bad'),409)
    sql(f"UPDATE runs SET result='\"Restored fixture\"'::jsonb WHERE id='{good}'")
    sql(f"UPDATE runs SET state='unknown',finished_at=NULL WHERE id='{good}'")
    expect_status(lambda:retry(id,'bad'),409)
    sql(f"UPDATE runs SET state='succeeded',finished_at=now() WHERE id='{good}'")
    api('/tasks/'+c['id']+'/archive','POST',dict(expected_revision=api('/tasks/'+c['id'])['revision']))
    expect_status(lambda:retry(id,'bad'),409)
    check('Missing retained results, unknown execution and archived tasks block manual recovery')

    c=config('manual-permission',[node('source',fail_first=1)])
    id=start(c);assert ended(id)['state']=='failed'
    invite=api('/invites','POST',dict(role='operator'))['token']
    username='manual-'+uuid.uuid4().hex[:8]
    call(context['base'],'/api/v1/invites/accept','POST',dict(token=invite,username=username,password='fixture-password'))
    actor=call(context['base'],'/api/v1/session','POST',dict(username=username,password='fixture-password'))['token']
    actor_id=context['global_api']('/session',token=actor)['id']
    api('/members/'+actor_id,'PUT',dict(role='viewer'))
    expected=detail(id,'source')['node']['execution_id']
    expect_status(lambda:api('/runs/'+id+'/nodes/source/retry','POST',dict(execution_id=expected),token=actor,gateway=context['other']),403)
    api('/members/'+actor_id,'PUT',dict(role='operator'))
    reply=api('/runs/'+id+'/nodes/source/retry','POST',dict(execution_id=expected),token=actor,gateway=context['other'])
    assert reply['parent_run_id']==id and ended(id)['state']=='succeeded'
    check('Viewer/downgraded credentials cannot retry across Gateways; restored Read/write permission can retry the same Run')

    # Historical frozen binding plus a live Session lock: reject before reserving a new attempt.
    c=config('manual-session-lock',[node('source',fail_first=1)])
    id=start(c);assert ended(id)['state']=='failed'
    ordinary=dict(version=1,id='manual-session-blocker',name='Session blocker',runner=a['id'],agent='mock-events',cwd=a['cwd'],prompt='Synthetic holder',schedule=None)
    api('/tasks/'+ordinary['id'],'PUT',dict(config=ordinary,expected_revision=None))
    blocker=api('/tasks/'+ordinary['id']+'/runs','POST',{})['id'];ended(blocker)
    sql("UPDATE runs SET config=jsonb_set(config,'{session}','{\"id\":\"synthetic-lock\"}'),state='running',finished_at=NULL WHERE id='"+blocker+"'")
    sql("UPDATE runs SET config=jsonb_set(config,'{dag,nodes,0,session}','{\"id\":\"synthetic-lock\"}') WHERE id='"+id+"'")
    sql("UPDATE dag_run_nodes SET config=jsonb_set(config,'{session}','{\"id\":\"synthetic-lock\"}') WHERE parent_run_id='"+id+"' AND node_id='source'")
    try:
        try:retry(id,'source')
        except APIError as error:assert error.status==409 and 'Session is currently in use' in str(error)
        else:raise AssertionError('busy Session accepted a manual attempt')
        assert len(detail(id,'source')['attempts'])==1
    finally:sql("UPDATE runs SET state='interrupted',finished_at=now() WHERE id='"+blocker+"'")
    check('Session lock is rechecked transactionally before creating a manual attempt')
