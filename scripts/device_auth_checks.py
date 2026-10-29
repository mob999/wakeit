"""Device-flow integration contracts against two real Gateway processes."""
import hashlib,json,time,urllib.request,uuid,os,pathlib,subprocess,tempfile,re
from integration_support import ROOT,BIN
from integration_support import call,expect_status,APIError

def verify(api,base,other,sql,tokens,check,ws):
    workspace=ws.workspace
    intent={'workspace_id':workspace,'name':'device-flow-fixture','os':'test','register_runner':True,'allowed_roots':['/tmp/测试 workspace']}
    def create(i=None):return call(base,'/api/v1/oauth/device/code','POST',i or intent)
    def browser(token):return {'Cookie':'wakeit_session='+token,'X-Wakeit-Request':'1','Origin':base}
    def decide(g,token,allow=True,gateway=other):return call(gateway,'/api/v1/device-authorizations/'+g['user_code']+('/approve' if allow else '/deny'),'POST',{},headers=browser(token))
    def poll(g):return call(base,'/api/v1/oauth/token','POST',{'grant_type':'urn:ietf:params:oauth:grant-type:device_code','device_code':g['device_code']})
    def error(fn,code):
        try:fn()
        except APIError as e:assert e.body['error']==code,(e.status,e.body);return
        raise AssertionError('expected '+code)
    def reset(g):sql("UPDATE device_grants SET last_poll=NULL WHERE device_hash='"+hashlib.sha256(g['device_code'].encode()).hexdigest()+"'")
    g=create();assert len(g['user_code'])==8 and g['expires_in']==600 and g['interval']==5
    assert call(base,'/api/v1/meta')['public_url']==base
    row=sql("SELECT intent::text FROM device_grants WHERE user_hash='"+hashlib.sha256(g['user_code'].encode()).hexdigest()+"'");assert json.loads(row)==intent
    error(lambda:poll(g),'authorization_pending');error(lambda:poll(g),'slow_down')
    expect_status(lambda:call(other,'/api/v1/device-authorizations/'+g['user_code'],token=tokens['operator']),401)
    expect_status(lambda:call(other,'/api/v1/device-authorizations/ZZZZZZZZ',headers=browser(tokens['operator'])),404)
    expect_status(lambda:decide(g,tokens['viewer']),403)
    expect_status(lambda:call(base,'/api/v1/device-authorizations/'+g['user_code']+'/approve','POST',{},headers={'Cookie':'wakeit_session='+tokens['operator']}),403)
    assert decide(g,tokens['operator'])['state']=='approved'
    assert decide(g,tokens['operator'])['state']=='approved'
    expect_status(lambda:decide(g,tokens['viewer'],False),409)
    reset(g);key=poll(g)['access_token'];assert call(other,'/api/v1/session',token=key)['username']=='operator'
    error(lambda:poll(g),'invalid_grant')
    # API keys cannot be supplied in a browser cookie to approve another request.
    another=create();expect_status(lambda:decide(another,key),403)
    decide(another,tokens['operator'],False);error(lambda:poll(another),'access_denied')
    expired=create();sql("UPDATE device_grants SET expires_at=now()-interval '1 second' WHERE user_hash='"+hashlib.sha256(expired['user_code'].encode()).hexdigest()+"'")
    error(lambda:poll(expired),'expired_token');expect_status(lambda:decide(expired,tokens['operator']),400)
    viewer=create(dict(intent,register_runner=False,allowed_roots=[]));decide(viewer,tokens['viewer']);viewer_key=poll(viewer)['access_token'];assert call(base,'/api/v1/session',token=viewer_key)['role']=='viewer'
    expect_status(lambda:ws.call(base,'/runners/enrollment','POST',{},viewer_key),403)
    downgraded=create();decide(downgraded,tokens['operator']);sql(f"UPDATE workspace_members SET role='viewer' WHERE workspace_id='{workspace}' AND user_id=(SELECT id FROM users WHERE username='operator')")
    error(lambda:poll(downgraded),'access_denied');sql(f"UPDATE workspace_members SET role='operator' WHERE workspace_id='{workspace}' AND user_id=(SELECT id FROM users WHERE username='operator')")
    disabled=create();decide(disabled,tokens['operator']);sql("UPDATE users SET active=false WHERE username='operator'")
    error(lambda:poll(disabled),'access_denied');sql("UPDATE users SET active=true WHERE username='operator'")
    # No-store includes error responses and no raw code is persisted.
    req=urllib.request.Request(base+'/api/v1/device-authorizations/'+g['user_code'],headers=browser(tokens['operator']))
    with urllib.request.urlopen(req) as r:assert r.headers['Cache-Control']=='no-store'
    assert sql("SELECT count(*) FROM device_grants WHERE device_hash='"+g['device_code']+"' OR user_hash='"+g['user_code']+"'")=='0'
    with tempfile.TemporaryDirectory(prefix='wakeit-connect-check-') as directory:
        root=pathlib.Path(directory)
        env={k:v for k,v in os.environ.items() if not k.startswith('WAKEIT_')}
        env.update(HOME=directory,USERPROFILE=directory,APPDATA=directory,XDG_CONFIG_HOME=str(root/'config'),WAKEIT_PASSWORD='wakeit-role-test-password',WAKEIT_WORKSPACE=workspace)
        def cli(*args,success=True,cwd=None,environment=None):
            p=subprocess.run([str(BIN),*args],env=environment or env,cwd=cwd or directory,text=True,capture_output=True)
            assert (p.returncode==0)==success,p.stderr
            return p
        # A machine without a browser can still authorize through the printed URL.
        empty=root/'empty-path';empty.mkdir()
        no_browser=dict(env,PATH=str(empty))
        process=subprocess.Popen([str(BIN),'login','--gateway',base],env=no_browser,cwd=directory,text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            lines=[]
            while len(lines)<10:
                line=process.stderr.readline()
                assert line,'CLI ended without a verification URL'
                lines.append(line)
                match=re.search(r'user_code=([A-Z2-9]{8})',line)
                if match:break
            assert match,'verification URL was not printed'
            decide({'user_code':match.group(1)},tokens['viewer'])
            stdout,stderr=process.communicate(timeout=20)
            assert process.returncode==0,stderr
            assert 'Open the URL above in your browser' in stderr
            assert 'access_token' not in stdout and 'device_code' not in stdout
        finally:
            if process.poll() is None:process.kill();process.wait()
        cli('login','--gateway',base,'--username','operator')
        listing=json.loads(cli('connection','list').stdout);name=listing['current']
        nested=root/'different-directory';nested.mkdir()
        assert isinstance(json.loads(cli('--json','task','list',cwd=nested).stdout),list)
        cli('--gateway',other,'task','list',success=False)
        cli('connection','use',name)
        cli('connection','use','does-not-exist',success=False)
        token=ws.call(base,'/tokens','POST',{'name':'registration-fixture'},tokens['operator'])['token']
        file=root/'fixture.key';file.touch(mode=0o600);file.write_text(token)
        p=subprocess.run([str(ROOT/'target/debug/examples/registration_probe'),base,str(file),directory,workspace],text=True,capture_output=True)
        assert p.returncode==0,p.stderr
        # No service manager on PATH: registration and profiles survive the failed install.
        restricted=dict(env,PATH=str(empty))
        failed=cli('setup','--gateway',base,'--runner-name','no-service-'+uuid.uuid4().hex[:8],'--allow-root',directory,success=False,environment=restricted)
        assert 'autostart is unfinished' in failed.stderr,failed.stderr
        profile=json.loads(cli('connection','list').stdout);assert profile['current']==name
        config_files=list(root.rglob('runner.yaml'));assert len(config_files)==1
        before=config_files[0].read_bytes()
        cli('setup','--gateway',base,'--allow-root',directory,success=False,environment=restricted)
        assert config_files[0].read_bytes()==before
    check('CLI manual-browser fallback, Gateway-scoped profiles across directories, signed registration recovery, name conflicts, rejected identity takeover and resumable service-manager failure')
    # Revoke the newly issued personal key and check the other Gateway immediately.
    rows=ws.call(base,'/tokens',token=key); own=next(t for t in rows if t['id']==hashlib.sha256(key.encode()).hexdigest())
    ws.call(base,'/tokens/'+own['id'],'DELETE',token=key);expect_status(lambda:call(other,'/api/v1/session',token=key),401)
    check('device authorization: Operator enrollment, Viewer CLI-only, Web session/CSRF, cross-Gateway single exchange, denial, expiry, polling backoff, demotion, disablement, no-store and revocation')
