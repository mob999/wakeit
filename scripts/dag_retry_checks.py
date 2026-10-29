"""Retry acceptance on disposable workspace infrastructure; never uses real tasks."""
import concurrent.futures
import json
import uuid
from integration_support import APIError, expect_status, wait


def verify(api, a, b, check, context):
    def config(name, fail_first=1, retries=2, timeout='30s'):
        node=dict(id='source',name='Source',pre=[],runner=a['id'],agent='mock-events',cwd=a['cwd'],prompt=json.dumps(dict(retry_fixture=uuid.uuid4().hex,fail_first=fail_first,text='Retried report')),timeout='10s',max_retries=retries)
        after=dict(node,id='after',name='After',pre=['source'],prompt='{{ nodes.source.result }}',max_retries=0)
        other=dict(node,id='independent',name='Independent',pre=[],prompt='Independent report',max_retries=0)
        return dict(version=1,id=name,name=name,schedule=None,enabled=True,execution=dict(timeout=timeout,start_grace='15s',overlap='skip',misfire='latest'),dag=dict(nodes=[node,after,other]))
    def start(c):
        api('/tasks/'+c['id'],'PUT',dict(config=c,expected_revision=None))
        return api('/tasks/'+c['id']+'/runs','POST',{})['id']
    def ended(id):
        return wait(lambda:(r if (r:=api('/runs/'+id))['finished_at'] else None),'retry graph ends',timeout=50)
    def waiting(id):
        return wait(lambda:next((n for n in api('/runs/'+id)['nodes'] if n.get('retry_at')),None),'retry is reserved',timeout=25)
    id=start(config('retry-once'))
    node=waiting(id)
    current=api('/runs/'+id)
    assert not current['finished_at'] and next(n for n in current['nodes'] if n['id']=='after')['state']=='waiting',current
    detail=api('/runs/'+id+'/nodes/source')
    assert len(detail['attempts'])==2 and detail['attempts'][0]['state']=='failed' and detail['execution']['state']=='queued',detail
    target=node['execution_id']
    counts=context['sql']("SELECT count(*) FROM outbox WHERE message_id='start:"+target+"'")
    assert str(counts).strip()=='0',counts
    expect_status(lambda:api('/runs/'+id+'/nodes/source/retry','POST',dict(execution_id=target),workspace=b['workspace'],token=b['token']),404)
    def accelerate(gateway):
        try:return api('/runs/'+id+'/nodes/source/retry','POST',dict(execution_id=target),**({'gateway':gateway} if gateway else {}))
        except APIError as e:assert e.status in [404,409];return None
    with concurrent.futures.ThreadPoolExecutor(2) as pool:
        replies=list(pool.map(accelerate,[context['other'],None]))
    assert any(replies)
    final=ended(id);assert final['state']=='succeeded',final
    detail=api('/runs/'+id+'/nodes/source')
    assert len(detail['attempts'])==2 and detail['execution']['id']==target,detail
    old=detail['attempts'][0]['id']
    assert api('/runs/'+id+'/nodes/source?execution='+old)['execution']['state']=='failed'
    expect_status(lambda:api('/runs/'+id+'/nodes/source?execution='+a['run']),404)
    assert next(n for n in final['nodes'] if n['id']=='after')['state']=='succeeded'
    check('Retries reserve before AND skip, preserve attempts, accelerate across Gateways and reject wrong ownership')

    id=start(config('retry-budget',fail_first=10,retries=1))
    final=ended(id)
    assert final['state']=='failed',final
    assert next(n for n in final['nodes'] if n['id']=='after')['state']=='skipped'
    assert len(api('/runs/'+id+'/nodes/source')['attempts'])==2
    check('Retry budget exhaustion finally skips descendants without repeating independent branches')

    id=start(config('retry-cancel',fail_first=10))
    node=waiting(id);api('/runs/'+id+'/cancel','POST',{})
    final=ended(id);assert final['state']=='cancelled',final
    assert api('/runs/'+id+'/nodes/source')['execution']['state']=='cancelled'
    expect_status(lambda:api('/runs/'+id+'/nodes/source/retry','POST',dict(execution_id=node['execution_id'])),409)
    check('Graph cancellation settles un-dispatched retry and forbids later acceleration')

    c=config('retry-total-timeout',fail_first=10,timeout='2s')
    id=start(c);final=ended(id);assert final['state']=='timed_out',final
    assert len(api('/runs/'+id+'/nodes/source')['attempts'])<=2
    check('Graph deadline includes retry delay and cannot be reset by another attempt')
