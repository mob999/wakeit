"""Generic extension contract checks against disposable Gateway and Runner processes."""
import datetime
import json
import pathlib
import time
import uuid
from integration_support import ROOT, BIN, command, wait, expect_status


def verify(c):
    api,runner,check=c['api'],c['runner_id'],c['check']
    import subprocess,os
    binary=ROOT/'target/debug/examples'/('generic.exe' if os.name=='nt' else 'generic')
    assert binary.is_file(),'build the Rust SDK generic example before service acceptance'
    native=subprocess.Popen([str(binary)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,cwd=c['cwd'])
    try:
        def send(value):native.stdin.write(json.dumps({'jsonrpc':'2.0',**value})+'\n');native.stdin.flush()
        send({'id':'initialize','method':'initialize','params':{'protocol_version':1}})
        initialized=json.loads(native.stdout.readline());assert initialized['result']['protocol_version']==1
        send({'id':'invoke','method':'invoke','params':{'id':str(uuid.uuid4()),'operation_id':str(uuid.uuid4()),'instance_id':str(uuid.uuid4()),'kind':'action','handler':'echo','input':{'text':'native'},'deadline':(datetime.datetime.now(datetime.timezone.utc)+datetime.timedelta(seconds=20)).isoformat(),'actor':{'kind':'test','id':None,'roles':[]},'expected_revision':None,'causation_id':None}})
        while True:
            message=json.loads(native.stdout.readline())
            if message.get('method')=='config.get':send({'id':message['id'],'result':{'value':{'prefix':'native-contract'},'revision':1}})
            elif message.get('id')=='invoke':assert message['result']['settings']['prefix']=='native-contract';break
        native.stdin.close();assert native.wait(timeout=5)==0
    finally:
        if native.poll() is None:native.kill();native.wait()
    check('native Rust SDK uses the same bidirectional process contract as the TypeScript SDK')

    prefix=f'/runners/{runner}/plugins'
    def control(action,**params):return api(prefix+'/control','POST',dict(action=action,params=params))
    catalog=wait(lambda:r if (r:=control('catalog'))['packages'] else None,'extension package discovery')
    package=next(p for p in catalog['packages'] if 'generic' in p.get('extensions',{}))
    declaration=package['extensions']['generic']
    instance=control('create',source=package['source'],extension='generic',name='Generic acceptance',config={'prefix':'initial'},grants=declaration['permissions'])
    identity=instance['id']
    def instance_now():return next(i for i in control('catalog')['instances'] if i['id']==identity)
    update={'operation_id':str(uuid.uuid4()),'expected_revision':instance['revision'],'config':{'prefix':'configured'},'secrets':{'fixture':{'operation':'replace','value':'synthetic-credential"\\line'}}}
    saved=control('configure',instance_id=identity,update=update)
    assert control('configure',instance_id=identity,update=update)==saved
    expect_status(lambda:control('configure',instance_id=identity,update={**update,'config':{'prefix':'different'}}),400)
    current=instance_now();assert current['config']['prefix']=='configured' and current['credential_status']['fixture']
    assert 'synthetic-credential' not in json.dumps(current)
    expect_status(lambda:control('configure',instance_id=identity,update={**update,'operation_id':str(uuid.uuid4())}),400)
    control('enable',instance_id=identity)
    wait(lambda:next((i for i in api(prefix)['instances'] if i['id']==identity and i['enabled']),None),'extension activation advertised')
    check('generic extension instance, revisioned settings and write-only credentials')
    second=control('create',source=package['source'],extension='generic',name='Isolated instance',config={'prefix':'separate'},grants=[])
    assert second['config']=={'prefix':'separate'} and not second['credential_status']['fixture']
    viewer_catalog=c['ws'].call(c['base'],prefix+'/control','POST',{'action':'catalog','params':{}},c['role_tokens']['viewer'])
    assert all('config' not in i for i in viewer_catalog['instances'])
    expect_status(lambda:c['ws'].call(c['base'],prefix+'/control','POST',{'action':'enable','params':{'instance_id':identity}},c['role_tokens']['operator']),403)
    expect_status(lambda:control('configure',instance_id=identity,update={**update,'operation_id':str(uuid.uuid4()),'expected_revision':current['revision'],'secrets':{'fixture':{'operation':'replace','value':''}}}),400)
    check('multiple instances isolate settings and credentials; member roles constrain device management')

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
    import urllib.request
    pending=c['task']('sdk-pending-save');pending['name']='wait-for-gate'
    url=c['base']+c['ws'].path('/tasks/sdk-pending-save')
    request=urllib.request.Request(url,data=json.dumps({'config':pending,'expected_revision':None}).encode(),method='PUT',headers={'Authorization':'Bearer '+c['token'],'Content-Type':'application/json','Idempotency-Key':str(uuid.uuid4())})
    with urllib.request.urlopen(request,timeout=10) as response:
        assert response.status==202
        operation=json.load(response)['id']
    wait(lambda:any(i['kind']=='hook' and i['handler']=='save' and i['state']=='running' for i in control('activity',instance_id=identity)['items']),'durable Gateway Hook started')
    cancelled=api('/plugins/operations/'+operation,'POST',{'action':'cancel'})
    assert cancelled['state']=='cancelled'
    wait(lambda:not any(i['kind']=='hook' and i['handler']=='save' and i['state']=='running' for i in control('activity',instance_id=identity)['items']),'Gateway Hook cancellation propagated')
    expect_status(lambda:api('/tasks/sdk-pending-save'),404)
    check('accepted business operations expose receipts and cancellation prevents a later commit')

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
    logs=control('activity',instance_id=identity)['logs']
    assert any('[redacted]' in entry.get('message','') for entry in logs)
    assert 'synthetic-credential' not in json.dumps(logs)

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
    call=str(uuid.uuid4());current=instance_now()
    denied=c['ws'].call(c['base'],prefix+'/control','POST',{'action':'invoke','params':{'invocation':dict(id=call,operation_id=call,instance_id=identity,kind='action',handler='oauth',input={},deadline=(datetime.datetime.now(datetime.timezone.utc)+datetime.timedelta(seconds=20)).isoformat(),expected_revision=current['revision'],actor={'kind':'user','id':None,'roles':[]},causation_id=None)}},c['role_tokens']['operator'])
    receipt=wait(lambda:r if (r:=api('/plugins/operations/'+denied['id']))['state']=='failed' else None,'operator account authorization denied')
    assert 'administrator' in str(receipt['error'])

    sample=c['cwd']/'sdk-sample.txt';sample.write_text('synthetic-file-data')
    operation,_=invoke('host-queries',{'path':str(sample)})
    receipt=wait(lambda:r if (r:=api('/plugins/operations/'+operation))['state'] in ['succeeded','failed'] else None,'host API domain queries')
    assert receipt['state']=='succeeded',receipt.get('error')
    assert receipt['result']['chunk']['data']=='c3ludA=='
    assert receipt['result']['models']['models'][0]['id']=='synthetic'
    artifact=receipt['result']['artifact']['id']
    def artifact_content():
        request=urllib.request.Request(c['base']+c['ws'].path('/plugins/artifacts/'+artifact+'/download'),headers={'Authorization':'Bearer '+c['token']})
        with urllib.request.urlopen(request,timeout=10) as response:return response.read()
    assert wait(artifact_content,'registered artifact download')==b'synthetic-file-data'

    check('typed business queries, query Provider and bounded authorized file reads')
    # Local HTTP provider only; no real mail, identity, or third-party service is involved.
    from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
    import threading,urllib.parse,urllib.request
    class Provider(BaseHTTPRequestHandler):
        def log_message(self,*args):pass
        def do_POST(self):
            data=urllib.parse.parse_qs(self.rfile.read(int(self.headers.get('Content-Length','0'))).decode())
            if self.path=='/device':result={'device_code':'synthetic-device','user_code':'ABC-123','verification_uri':provider_url+'/verify','interval':1,'expires_in':60}
            else:result={'access_token':'synthetic-access-token','refresh_token':'synthetic-refresh-token','expires_in':3600,'token_type':'Bearer'}
            payload=json.dumps(result).encode();self.send_response(200);self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(payload)
        def do_GET(self):
            if self.path.startswith('/authorize?'):
                q=urllib.parse.parse_qs(urllib.parse.urlsplit(self.path).query)
                assert q['code_challenge_method']==['S256'] and len(q['code_challenge'][0])>=40
                target=q['redirect_uri'][0]+'?'+urllib.parse.urlencode({'state':q['state'][0],'code':'synthetic-code'})
                self.send_response(302);self.send_header('Location',target);self.end_headers()
            else:self.send_response(200);self.end_headers();self.wfile.write(b'Authorized')
    server=ThreadingHTTPServer(('127.0.0.1',0),Provider)
    provider_url='http://127.0.0.1:'+str(server.server_address[1])
    thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
    try:
        for mode in ['device_code','authorization_code']:
            operation,_=invoke('oauth',dict(mode=mode,slot='oauth',client_id='synthetic-client',token_url=provider_url+'/token',authorization_url=provider_url+'/authorize',device_authorization_url=provider_url+'/device',scope='synthetic'))
            interaction=wait(lambda:next((i for i in control('activity',instance_id=identity).get('interactions',[]) if i['request']['kind'] in ['open_url','device_code']),None),'synthetic authorization UI')
            with urllib.request.urlopen(interaction['request']['url'],timeout=10) as response:assert response.status==200
            control('interaction',instance_id=identity,id=interaction['id'],response={'confirmed':True})
            receipt=wait(lambda:r if (r:=api('/plugins/operations/'+operation))['state'] in ['succeeded','failed'] else None,'synthetic OAuth completed')
            assert receipt['state']=='succeeded',receipt.get('error')
            assert receipt['result']=={'authorized':True}
    finally:server.shutdown();server.server_close()
    assert instance_now()['credential_status']['oauth']
    assert 'synthetic-access-token' not in json.dumps(api('/plugins/operations?instance='+identity))
    check('generic OAuth device-code and authorization-code/PKCE with device-only token storage')
    publish=binding('hook','result.publish.prepare','publish')
    operation,_=invoke('postprocess-mode',{'reject':True})
    wait(lambda:api('/plugins/operations/'+operation)['state']=='succeeded','synthetic postprocess policy')
    time.sleep(2.5)
    rejected=c['trigger']('sdk-generic')
    failed=c['final'](rejected,'failed')
    assert failed['execution_snapshot']['failure_phase']=='postprocess'
    original_calls=(c['remote']/'calls.log').read_text().count('sync '+rejected)
    assert original_calls==1
    operation,_=invoke('postprocess-mode',{'reject':False})
    wait(lambda:api('/plugins/operations/'+operation)['state']=='succeeded','updated postprocess policy')
    recovery=api('/runs/'+rejected+'/result/retry','POST',{})
    receipt=wait(lambda:r if (r:=api('/plugins/operations/'+recovery['id']))['state'] in ['succeeded','failed'] else None,'result-only processing retry')
    assert receipt['state']=='succeeded',receipt.get('error')
    assert c['final'](rejected)['result'].endswith('prepared:original\n')
    assert (c['remote']/'calls.log').read_text().count('sync '+rejected)==1
    check('result processing preserves native output and can recover without rerunning the Agent')
    api('/plugins/bindings','POST',{**publish,'enabled':False})


    for b in (observer,save_hook,prepared_hook):api('/plugins/bindings','POST',{**b,'enabled':False})
    control('disable',instance_id=identity)
    assert instance_now()['state']=='disabled'
    check('extension disable is independent of installed Agent resources')
