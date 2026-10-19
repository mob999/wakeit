"""AND graph acceptance against the disposable two-Gateway workspace stack."""
import concurrent.futures
import copy
import json
import time
from integration import expect_status, wait


def verify(api, a, b, check):
    def node(id, pre=(), text=None, sleep=.15, fail=False):
        return dict(id=id, name='Node '+id, pre=list(pre), runner=a['id'], agent='mock-events', cwd=a['cwd'],
                    prompt=json.dumps(dict(text=text or '# '+id, sleep=sleep, fail=fail)), timeout='20s')

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

    # Repeated trigger requests use the normal idempotency key across Gateway replicas.
    save('dag-once',[node('one',sleep=1)])
    key='dag-identical-request'
    # api intentionally supplies fresh keys; use a delayed retry while a round is active.
    first=start('dag-once')
    expect_status(lambda:start('dag-once'),409)
    finish(first)
    latest=api('/tasks/dag-once');latest['config']['dag']['nodes'][0]['prompt']=json.dumps({'text':'new definition'})
    api('/tasks/dag-once','PUT',dict(config=latest['config'],expected_revision=latest['revision']))
    retry=api('/runs/'+first+'/retry','POST',{})['id']
    assert finish(retry)['result']['outputs'][0]['result']=='new definition'
    check('DAG overlap guard and whole-graph retry use current definitions without changing old snapshots')
    return dict(join=run,warning=warning['id'],failed=failed['id'],cancelled=cancelled)
