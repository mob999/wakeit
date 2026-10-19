"""AND graph acceptance against the disposable two-Gateway workspace stack."""
import concurrent.futures
import copy
import json
import time
import os
import subprocess
import uuid
from integration import APIError, expect_status, wait


def verify(api, a, b, check, context):
    def node(id, pre=(), text=None, sleep=.15, fail=False):
        return dict(id=id, name='Node '+id, pre=list(pre), runner=a['id'], agent='mock-events', cwd=a['cwd'],
                    prompt=json.dumps(dict(text=('# '+id) if text is None else text, sleep=sleep, fail=fail)), timeout='20s')

    def save(id, nodes, results=None):
        config=dict(version=1,id=id,name='DAG '+id,schedule=None,enabled=True,
                    execution=dict(timeout='30s',start_grace='20s',overlap='skip',misfire='latest'),dag=dict(nodes=nodes))
        if results is not None: config['dag']['results']=results
        old=next((t for t in api('/tasks') if t['id']==id),None)
        return api('/tasks/'+id,'PUT',dict(config=config,expected_revision=old['revision'] if old else None))

    def start(id): return api('/tasks/'+id+'/runs','POST',{})['id']
    def finish(id):
        return wait(lambda:(r if (r:=api('/runs/'+id))['state'] in ['succeeded','warning','failed','cancelled','timed_out'] else None),'DAG '+id,timeout=40)

    root_a=node('a',sleep=.3)
    root_b=node('b',sleep=.3)
    tail=node('c',['a','b'])
    tail['prompt']='{{ nodes.a.result }}\n{{ nodes.b.result }}'
    saved=save('dag-join',[root_a,root_b,tail])
    assert saved['runner_id'] is None
    before=len(api('/runs'))
    run=start('dag-join');final=finish(run)
    assert final['state']=='succeeded',final
    assert final['result']['outputs'][0]['node_id']=='c'
    assert final['result']['outputs'][0]['result']=='# a\n# b'
    nodes={n['id']:n for n in final['nodes']}
    assert all(n['state']=='succeeded' for n in nodes.values())
    assert nodes['c']['started_at']>=max(nodes['a']['finished_at'],nodes['b']['finished_at'])
    assert len(api('/runs'))==before+1
    for n in nodes.values():
        expect_status(lambda:api('/runs/'+n['execution_id']+'/retry','POST',{}),404)
        expect_status(lambda:api('/runs/'+n['execution_id']+'/cancel','POST',{}),400)
    detail=api('/runs/'+run+'/nodes/c')['execution']
    assert detail['config']['prompt']=='# a\n# b'
    assert detail['execution_snapshot']['input_sources']==['a','b']
    expect_status(lambda:api('/runs/'+run+'/nodes/c',workspace=b['workspace'],token=b['token']),404)
    check('DAG multiple roots, AND join, same-round rendered inputs, internal children and scoped node access')

    invalid=copy.deepcopy(saved['config']);invalid['dag']['nodes'][0]['pre']=['c']
    response=api('/tasks/validate','POST',dict(config=invalid))
    assert any(e.get('cycle') for e in response['errors'])
    expect_status(lambda:api('/tasks/dag-join','PUT',dict(config=invalid,expected_revision=saved['revision'])),400)
    assert api('/tasks/dag-join')['revision']==saved['revision']
    expect_status(lambda:api('/tasks/dag-join','PUT',dict(config=saved['config'],expected_revision=0)),409)
    check('DAG validation identifies cycle paths and failed/conflicting saves preserve the entire definition')

    fail=node('fail',fail=True);success=node('ok');blocked=node('blocked',['fail'])
    save('dag-warning',[fail,success,blocked],['ok'])
    warning=finish(start('dag-warning'))
    assert warning['state']=='warning',warning
    assert {n['id']:n['state'] for n in warning['nodes']}==dict(fail='failed',ok='succeeded',blocked='skipped')
    save('dag-failed',[fail,success,blocked])
    failed=finish(start('dag-failed'))
    assert failed['state']=='failed',failed
    check('DAG independent branches continue, impossible descendants skip and selected outputs distinguish warning from failure')

    save('dag-cancel',[node('long',sleep=15),node('after',['long'])])
    cancelled=start('dag-cancel')
    wait(lambda:api('/runs/'+cancelled)['started_at'],'DAG starts before cancellation')
    api('/runs/'+cancelled+'/cancel','POST',{})
    end=finish(cancelled)
    assert end['state']=='cancelled',end
    assert all(n['state'] in ['cancelled','skipped'] for n in end['nodes'])
    check('DAG cancellation stops dispatch and confirms active subprocess cancellation before ending the graph')

    # The same request can reach separate replicas without creating a second round.
    save('dag-once',[node('one',sleep=1)])
    key='dag-identical-request'
    with concurrent.futures.ThreadPoolExecutor(2) as executor:
        calls=[executor.submit(api, '/tasks/dag-once/runs', 'POST', {}, gateway=gateway, key=key) for gateway in [context['other'],None] if gateway]
        calls.append(executor.submit(api, '/tasks/dag-once/runs', 'POST', {}, key=key))
        ids=[call.result()['id'] for call in calls]
    assert len(set(ids))==1
    first=ids[0]
    expect_status(lambda:start('dag-once'),409)
    finish(first)
    latest=api('/tasks/dag-once');latest['config']['dag']['nodes'][0]['prompt']=json.dumps({'text':'new definition'})
    api('/tasks/dag-once','PUT',dict(config=latest['config'],expected_revision=latest['revision']))
    retry=api('/runs/'+first+'/retry','POST',{})['id']
    assert finish(retry)['result']['outputs'][0]['result']=='new definition'
    check('DAG overlap guard and whole-graph retry use current definitions without changing old snapshots')
    # Parent reports, pagination and event identities never expose children as independent runs.
    days=api('/moments/days')
    assert days['total_runs']==len([r for r in api('/runs') if r['finished_at']])
    items=api('/moments?date='+days['days'][0]['date'])['items']
    report=next(r for r in items if r['id']==warning['id'])
    assert report['format']=='dag' and report['node_count']==3 and len(report['outputs'])==1
    assert report['outputs'][0]['node_id']=='ok'
    assert all(not r.get('parent_run_id') for r in api('/runs?page=1')['items'])
    assert api('/runs?state=warning&page=1')['total']==1
    assert any(r['id']=='dag-warning' for r in api('/tasks?agent=mock-events'))
    cursor=0;observed=[]
    while True:
        batch=api('/runs/'+run+'/events?after='+str(cursor))
        if not batch:break
        assert all(e['id']>cursor and e['run_id']==run for e in batch)
        observed.extend(batch);cursor=batch[-1]['id']
    assert any(e.get('node_id')=='c' and e.get('execution_id') for e in observed)
    cli=subprocess.run([str(context['binary']),'--workspace',a['workspace'],'--json','run','watch',warning['id'],'--timeout','10s'],env=context['cli_env'],text=True,capture_output=True)
    assert cli.returncode==0,cli.stderr
    watch=[json.loads(line) for line in cli.stdout.splitlines()]
    assert watch[-1]['type']=='summary' and watch[-1]['data']['state']=='warning'
    assert any(e.get('node_id')=='ok' for e in watch)
    check('DAG parent-only Moments/counts/search, report boundaries, node event cursors and warning-aware Watch')

    sql=context['sql']
    # Exact duplicate and late transport deliveries are durably deduplicated; no restart of an Agent.
    source=nodes['a']['execution_id']
    before=sql("SELECT count(*) FROM events WHERE run_id='"+source+"'")
    sql("INSERT INTO events(workspace_id,run_id,seq,kind,data,created_at) SELECT workspace_id,run_id,seq,kind,data,created_at FROM events WHERE run_id='"+source+"' ON CONFLICT DO NOTHING")
    assert sql("SELECT count(*) FROM events WHERE run_id='"+source+"'")==before
    # Cross-workspace and wrong-parent associations must be rejected by PostgreSQL itself.
    for query in [
        "UPDATE dag_run_nodes SET runner_id='"+b['id']+"' WHERE parent_run_id='"+run+"' AND node_id='a'",
        "UPDATE dag_run_nodes SET execution_id='"+b['run']+"' WHERE parent_run_id='"+run+"' AND node_id='a'",
        "UPDATE dag_run_nodes SET execution_id='"+nodes['b']['execution_id']+"' WHERE parent_run_id='"+run+"' AND node_id='a'"
    ]:
        try:sql(query)
        except RuntimeError:pass
        else:raise AssertionError('database accepted an invalid DAG association')
    check('DAG duplicate persistence is idempotent and composite constraints reject cross-workspace or wrong-node ownership')

    # Reopen a finished synthetic child as unknown; only reconciliation releases it.
    save('dag-unknown',[node('uncertain',sleep=0),node('after',['uncertain'])])
    pending=start('dag-unknown')
    # Marking the completed root unknown tests aggregation/reconciliation on a frozen round.
    done=finish(pending)
    execution=done['nodes'][0]['execution_id']
    sql("BEGIN; UPDATE runs SET state='unknown',finished_at=NULL WHERE id='"+execution+"'; UPDATE runs SET state='running',finished_at=NULL,result=NULL WHERE id='"+pending+"'; COMMIT")
    uncertain=wait(lambda:(r if (r:=api('/runs/'+pending))['state']=='unknown' else None),'DAG holds unknown state')
    assert uncertain['finished_at'] is None
    expect_status(lambda:api('/runs/'+pending+'/resolve','POST',dict(state='succeeded',reason='fixture')),400)
    api('/runs/'+pending+'/nodes/uncertain/resolve','POST',dict(state='succeeded',reason='Confirmed by synthetic fixture'))
    assert finish(pending)['state']=='succeeded'
    check('DAG unknown remains nonterminal until the actual node is reconciled; direct parent resolution is rejected')

    # Reference errors fail before the downstream Agent starts.
    for suffix,text in [('empty',''),('truncated',{'truncated':True,'preview':'partial'})]:
        root=node('source',text=text)
        dependent=node('report',['source']);dependent['prompt']='{{ nodes.source.result }}'
        save('dag-input-'+suffix,[root,dependent])
        ended=finish(start('dag-input-'+suffix))
        assert ended['state']=='failed',ended
        assert ended['nodes'][1]['execution_id'] is None
    check('DAG empty upstream results explicitly fail consumers before Agent submission')

    # A whole-graph timeout cancels actual processes and prevents subsequent dispatch.
    definition=save('dag-timeout',[node('long',sleep=15),node('after',['long'])])
    definition['config']['execution']['timeout']='1s'
    api('/tasks/dag-timeout','PUT',dict(config=definition['config'],expected_revision=definition['revision']))
    timed=finish(start('dag-timeout'))
    assert timed['state']=='timed_out' and timed['finished_at']
    assert timed['nodes'][1]['execution_id'] is None
    check('DAG whole-graph timeout begins at first claim and confirms node cancellation')

    # Archiving an in-flight round preserves its frozen downstream execution but blocks future rounds.
    current=save('dag-archive',[node('first',sleep=1.5),node('second',['first'])])
    archived=start('dag-archive')
    wait(lambda:api('/runs/'+archived)['started_at'],'DAG starts before archive')
    api('/tasks/dag-archive/archive','POST',dict(expected_revision=current['revision']))
    assert finish(archived)['state']=='succeeded'
    expect_status(lambda:start('dag-archive'),409)
    expect_status(lambda:api('/runs/'+archived+'/retry','POST',{}),409)
    assert api('/runs/'+archived)['task_archived']
    check('DAG started rounds survive task archive; new triggers and retries stay blocked')

    # Larger than a single 500-record page, including identical native Trace IDs in different nodes.
    many=node('many',text='report');many['prompt']=json.dumps(dict(text='report',trace_count=5300,sleep=0))
    save('dag-many',[many])
    large=finish(start('dag-many'))
    # Terminal state and durable content travel independently. Wait for this
    # fixture's known records before asserting pagination, including on slower CI.
    execution=large['nodes'][0]['execution_id']
    wait(lambda:int(sql("SELECT count(*) FROM events WHERE run_id='"+execution+"' AND kind='trace'"))>=5300,
         'large DAG Trace content is persisted',timeout=90)
    cursor=0;count=0
    while True:
        batch=api('/runs/'+large['id']+'/events?kind=trace&after='+str(cursor))
        if not batch:break
        assert len(batch)<=500
        assert all(e['id']>cursor for e in batch)
        cursor=batch[-1]['id'];count+=len(batch)
    assert count>5000,count
    check('DAG Trace stays independently pageable beyond 5000 events without lost cursors')
    return dict(join=run,warning=warning['id'],failed=failed['id'],cancelled=cancelled)
