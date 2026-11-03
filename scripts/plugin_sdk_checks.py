"""Generic extension contract checks against disposable Gateway and Runner processes."""
import datetime
import json
import pathlib
import time
import uuid
from integration_support import ROOT, BIN, command, wait, expect_status


def verify(c):
    api,runner,check=c['api'],c['runner_id'],c['check']
    prefix=f'/runners/{runner}/plugins'
    def control(action,**params):return api(prefix+'/control','POST',dict(action=action,params=params))
    catalog=wait(lambda:r if (r:=control('catalog'))['packages'] else None,'extension package discovery')
    package=next(p for p in catalog['packages'] if 'generic' in p.get('extensions',{}))
    declaration=package['extensions']['generic']
    instance=control('create',source=package['source'],extension='generic',name='Generic acceptance',config={'prefix':'initial'},grants=declaration['permissions'])
    identity=instance['id']
    def instance_now():return next(i for i in control('catalog')['instances'] if i['id']==identity)
    update={'operation_id':str(uuid.uuid4()),'expected_revision':instance['revision'],'config':{'prefix':'configured'},'secrets':{'fixture':{'operation':'replace','value':'synthetic-credential'}}}
    saved=control('configure',instance_id=identity,update=update)
    assert control('configure',instance_id=identity,update=update)==saved
    current=instance_now();assert current['config']['prefix']=='configured' and current['credential_status']['fixture']
    assert 'synthetic-credential' not in json.dumps(current)
    expect_status(lambda:control('configure',instance_id=identity,update={**update,'operation_id':str(uuid.uuid4())}),400)
    control('enable',instance_id=identity)
    wait(lambda:next((i for i in api(prefix)['instances'] if i['id']==identity and i['enabled']),None),'extension activation advertised')
    check('generic extension instance, revisioned settings and write-only credentials')
    def binding(kind,event,handler,position=0):
        return api('/plugins/bindings','POST',dict(id=str(uuid.uuid4()),instance_id=identity,runner_id=runner,kind=kind,event=event,handler=handler,enabled=True,filter={},position=position,failure_policy='reject',timeout_seconds=10,max_attempts=3,expires_seconds=3600,revision=0))
    observer=binding('event','run.finished','completed')
    save_hook=binding('hook','task.save.prepare','save')
    prepared_hook=binding('hook','execution.prepare','prepare')
    definition=c['task']('sdk-generic',prompt='original')
    c['apply'](definition)
    invalid=c['task']('sdk-rejected');invalid['name']='reject'
    expect_status(lambda:c['apply'](invalid),400)
    check('Gateway Hook allow and rejection before Task commit')
    # Polling distributes local hook bindings. Observe it explicitly before execution.
    time.sleep(2.5)
    run=c['trigger']('sdk-generic');result=c['final'](run)
    assert result['result'].endswith('prepared:original\n'),result.get('reason')
    deliveries=wait(lambda:rows if (rows:=api('/plugins/deliveries?instance='+identity)['items']) and any(d['state']=='succeeded' for d in rows) else None,'durable completion handler')
    first=next(d for d in deliveries if d['state']=='succeeded')
    assert api('/plugins/events/'+first['event_id'])['data']['result']==result['result']
    check('Runner input Hook, effective result and durable generic event delivery')
    replay=api('/plugins/deliveries/'+first['id'],'POST',{'action':'replay'})
    wait(lambda:any(d['id']==replay['id'] and d['state']=='succeeded' for d in api('/plugins/deliveries?instance='+identity)['items']),'explicit event replay')
    check('event replay has a separate delivery while preserving the original event')
    def invoke(handler,input):
        now=instance_now();call=str(uuid.uuid4())
        invocation=dict(id=call,operation_id=call,instance_id=identity,kind='action',handler=handler,input=input,deadline=(datetime.datetime.now(datetime.timezone.utc)+datetime.timedelta(seconds=30)).isoformat(),expected_revision=now['revision'],actor={'kind':'user','id':None,'roles':[]},causation_id=None)
        receipt=control('invoke',invocation=invocation)
        return receipt['id'],call
    operation,_=invoke('echo',{'text':'test'})
    receipt=wait(lambda:r if (r:=api('/plugins/operations/'+operation))['state']=='succeeded' else None,'typed action host API')
    assert receipt['result']['settings']['prefix']=='configured'
    operation,call=invoke('interact',{})
    interaction=wait(lambda:(a['interactions'][0] if a.get('interactions') else None) if (a:=control('activity',instance_id=identity)) else None,'structured action form')
    control('interaction',instance_id=identity,id=interaction['id'],response={'answer':'synthetic'})
    receipt=wait(lambda:r if (r:=api('/plugins/operations/'+operation))['state']=='succeeded' else None,'structured action completed')
    assert receipt['result']['answer']=={'answer':'synthetic'}
    operation,call=invoke('wait',{})
    wait(lambda:any(i['id']==call and i['state']=='running' for i in control('activity',instance_id=identity)['items']),'cancellable action started')
    control('cancel',instance_id=identity,invocation_id=call)
    wait(lambda:api('/plugins/operations/'+operation)['state'] in ['failed','cancelled'],'action cancelled')
    check('generic actions, structured interaction, reverse host API and cancellation')
    for b in (observer,save_hook,prepared_hook):api('/plugins/bindings','POST',{**b,'enabled':False})
    control('disable',instance_id=identity)
    assert instance_now()['state']=='disabled'
    check('extension disable is independent of installed Agent resources')
