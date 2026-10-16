#!/usr/bin/env python3
"""Real PostgreSQL + NATS integration tests. Uses only the Python standard library."""
import argparse, concurrent.futures, datetime, getpass, json, os, pathlib, shutil, signal, socket, subprocess, time, urllib.request, urllib.error, uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
BIN = ROOT / 'target/debug/wakeit'
MOCK = ROOT / 'target/debug/examples/mock_agent'
PROBE = ROOT / 'target/debug/examples/bus_probe'

class APIError(Exception):
    def __init__(self, status, body): self.status, self.body = status, body; super().__init__(f'{status}: {body}')

def port():
    with socket.socket() as sock: sock.bind(('127.0.0.1', 0)); return sock.getsockname()[1]

def call(base, path, method='GET', body=None, token=None, key=None, headers=None):
    headers = dict(headers or {})
    if token: headers['Authorization'] = f'Bearer {token}'
    if method == 'POST': headers['Idempotency-Key'] = key or str(uuid.uuid4())
    data = json.dumps(body).encode() if body is not None else None
    if data: headers['Content-Type'] = 'application/json'
    req = urllib.request.Request(base + path, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=20) as response: return json.load(response)
    except urllib.error.HTTPError as error:
        raise APIError(error.code, json.load(error)) from None

def wait(fn, description, timeout=25):
    deadline = time.monotonic() + timeout
    error = None
    while time.monotonic() < deadline:
        try:
            value = fn()
            if value: return value
        except (APIError, OSError) as e: error = str(e)
        time.sleep(.2)
    raise AssertionError(f'timed out: {description}; {error or "condition not reached"}')

def expect_status(fn, status):
    try: fn()
    except APIError as error: assert error.status == status, error; return
    raise AssertionError(f'expected HTTP {status}')

def command(args, env=None):
    result=subprocess.run([str(a) for a in args], cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode:
        raise RuntimeError(f'command failed: {args[0]}\n{result.stdout[-4000:]}\n{result.stderr[-4000:]}')
    return result.stdout

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--pg-bin', default=os.environ.get('PG_BIN'))
    parser.add_argument('--keep-running', action='store_true')
    parser.add_argument('--gateway-port', type=int, help='Optional fixed port for the local preview')
    parser.add_argument('--state-file', default='.local/integration-stack.json')
    args = parser.parse_args()
    pg_bin = pathlib.Path(args.pg_bin or command(['pg_config', '--bindir']).strip())
    for binary in [BIN, MOCK, PROBE]: assert binary.exists(), f'build first: cargo build --examples; missing {binary}'
    root = ROOT / '.local' / ('integration-' + uuid.uuid4().hex[:8]); root.mkdir(parents=True)
    pg_port, nats_port, ws_port, gateway_port, second_port = [port() for _ in range(5)]
    if args.gateway_port: gateway_port=args.gateway_port
    database = f'postgres://{getpass.getuser()}@127.0.0.1:{pg_port}/postgres'
    env = dict(os.environ, WAKEIT_DATABASE_URL=database, WAKEIT_PASSWORD='wakeit-integration-password', RUST_LOG='wakeit=info,async_nats=warn')
    processes, files, checks = [], [], []
    pg_started = False
    success = False
    def spawn(name, args):
        log = open(root / f'{name}.log', 'ab', buffering=0); files.append(log)
        process = subprocess.Popen([str(a) for a in args], cwd=ROOT, env=env, stdout=log, stderr=log)
        processes.append(process); return process
    def stop(process):
        if process.poll() is None:
            process.terminate()
            try: process.wait(timeout=5)
            except subprocess.TimeoutExpired: process.kill(); process.wait()
    def sql(query): return command([pg_bin/'psql', database, '-At', '-c', query]).strip()
    def check(name): checks.append(name); print(f'PASS {name}', flush=True)
    try:
        command([pg_bin/'initdb', '-D', root/'pg', '-A', 'trust', '--no-locale', '-E', 'UTF8'])
        command([pg_bin/'pg_ctl','-D',root/'pg','-l',root/'postgres.log','-o',f"-p {pg_port} -h 127.0.0.1 -k ''",'start']); pg_started=True
        auth_dir = root/'nats'
        command([BIN,'gateway','nats-init','--dir',auth_dir,'--port',nats_port,'--ws-port',ws_port])
        broker = spawn('nats',[shutil.which('nats-server'),'--config',auth_dir/'nats.conf'])
        config = {'version':1,'listen':f'127.0.0.1:{gateway_port}','public_url':f'http://127.0.0.1:{gateway_port}','database_url_env':'WAKEIT_DATABASE_URL','nats_url':f'nats://127.0.0.1:{nats_port}','nats_public_url':f'ws://127.0.0.1:{ws_port}','nats_auth_dir':str(auth_dir),'nats_replicas':1}
        config_file=root/'gateway.yaml'; config_file.write_text(json.dumps(config))
        second_config=root/'gateway-second.yaml'; second_config.write_text(json.dumps(dict(config,listen=f'127.0.0.1:{second_port}')))
        command([BIN,'gateway','migrate','--config',config_file],env)
        command([BIN,'gateway','bootstrap','--config',config_file,'--username','admin'],env)
        first=spawn('gateway',[BIN,'gateway','start','--config',config_file]); second=spawn('gateway-second',[BIN,'gateway','start','--config',second_config])
        base, other = config['public_url'],f'http://127.0.0.1:{second_port}'
        wait(lambda:call(base,'/health/ready'),'first Gateway ready')
        wait(lambda:call(other,'/health/ready'),'second Gateway ready')
        token=call(base,'/api/v1/session','POST',{'username':'admin','password':env['WAKEIT_PASSWORD']})['token']
        def api(path,method='GET',body=None,key=None):return call(base,'/api/v1'+path,method,body,token,key)
        assert call(other,'/api/v1/session',token=token)['username']=='admin'
        check('shared authentication across Gateway instances')
        expect_status(lambda:call(base,'/api/v1/tokens','POST',{'name':'csrf'},headers={'Cookie':f'wakeit_session={token}'}),403)
        check('cookie write CSRF protection')
        role_tokens={}
        for role in ['operator','viewer']:
            invite=api('/invites','POST',{'role':role})['token']
            call(base,'/api/v1/invites/accept','POST',{'token':invite,'username':role,'password':'wakeit-role-test-password'})
            expect_status(lambda:call(base,'/api/v1/invites/accept','POST',{'token':invite,'username':role+'2','password':'wakeit-role-test-password'}),400)
            role_tokens[role]=call(base,'/api/v1/session','POST',{'username':role,'password':'wakeit-role-test-password'})['token']
        assert call(base,'/api/v1/runners/enrollment','POST',{},role_tokens['operator'])['token']
        expect_status(lambda:call(base,'/api/v1/runners/enrollment','POST',{},role_tokens['viewer']),403)
        expect_status(lambda:call(base,'/api/v1/users',token=role_tokens['viewer']),403)
        check('member roles and single-use invitations')
        zones=api('/timezones');assert zones==sorted(set(zones)) and 'Asia/Shanghai' in zones and 'UTC' in zones
        assert call(base,'/api/v1/timezones',token=role_tokens['viewer'])==zones
        expect_status(lambda:call(base,'/api/v1/timezones'),401)
        admin_user=api('/session')['id']
        expect_status(lambda:api('/users/'+admin_user,'PUT',{'role':'operator','active':True}),409)
        sql("INSERT INTO users(id,username,password_hash,role,active) SELECT gen_random_uuid(),'inactive-fixture',password_hash,'viewer',false FROM users WHERE username='admin'")
        sql("INSERT INTO users(id,username,password_hash,role) SELECT gen_random_uuid(),'pagination-member-'||lpad(i::text,2,'0'),password_hash,'viewer' FROM users CROSS JOIN generate_series(0,22) i WHERE username='admin'")
        members1=api('/users?page=1&page_size=20');members2=api('/users?page=2&page_size=20')
        assert members1['total']==27 and members1['active_admins']==1 and len(members1['items'])==20 and len(members2['items'])==7
        assert not {m['id'] for m in members1['items']} & {m['id'] for m in members2['items']}
        expect_status(lambda:call(base,'/api/v1/users?page=1',token=role_tokens['operator']),403)
        expect_status(lambda:api('/users?page=0'),400)
        expect_status(lambda:api('/tokens','POST',{'name':'   '}),400)
        for i in range(23):api('/tokens','POST',{'name':f'pagination-token-{i:02}'})
        token1=api('/tokens?page=1&page_size=20');token2=api('/tokens?page=2&page_size=20')
        assert token1['total']==23 and len(token1['items'])==20 and len(token2['items'])==3
        assert not {t['id'] for t in token1['items']} & {t['id'] for t in token2['items']}
        assert all('created_at' in t and 'token' not in t for t in token1['items'])
        call(base,'/api/v1/tokens','POST',{'name':'viewer-fixture'},role_tokens['viewer'])
        own=call(base,'/api/v1/tokens?page=1',token=role_tokens['viewer']);assert own['total']==1 and own['items'][0]['name']=='viewer-fixture'
        call(base,'/api/v1/tokens/'+token1['items'][0]['id'],'DELETE',token=role_tokens['viewer'])
        assert api('/tokens?page=1')['total']==23
        assert all(t['expires_at'] is None for t in token1['items'])
        permanent=api('/tokens','POST',{'name':'no-expiration'})['token']
        assert call(other,'/api/v1/session',token=permanent)['id']==admin_user
        future=(datetime.datetime.now(datetime.timezone.utc)+datetime.timedelta(days=7)).isoformat()
        expiring=api('/tokens','POST',{'name':'with-expiration','expires_at':future})['token']
        explicit_null=api('/tokens','POST',{'name':'explicit-null','expires_at':None})['token']
        assert call(base,'/api/v1/session',token=explicit_null)['id']==admin_user
        rows={t['name']:t for t in api('/tokens')}
        assert rows['no-expiration']['expires_at'] is None and rows['explicit-null']['expires_at'] is None
        assert datetime.datetime.fromisoformat(rows['with-expiration']['expires_at'])==datetime.datetime.fromisoformat(future)
        assert call(base,'/api/v1/session',token=expiring)['id']==admin_user
        for invalid,status in [('2000-01-01T00:00:00Z',400), ('not-a-date',422)]:
            expect_status(lambda:api('/tokens','POST',{'name':'invalid-expiration','expires_at':invalid}),status)
        sql("UPDATE credentials SET expires_at=now()-interval '1 second' WHERE hash='"+rows['with-expiration']['id']+"'")
        expect_status(lambda:call(base,'/api/v1/session',token=expiring),401)
        assert call(other,'/api/v1/session',token=permanent)['id']==admin_user
        for name in ['no-expiration','with-expiration','explicit-null']:api('/tokens/'+rows[name]['id'],'DELETE')
        expect_status(lambda:call(base,'/api/v1/session',token=permanent),401)
        check('settings timezone catalog, member pagination, last-admin protection and private API Key pagination and expiry')
        enrollment=api('/runners/enrollment','POST',{})['token']; runner_dir=root/'runner'
        command([BIN,'--gateway',base,'runner','enroll','--name','test-device','--token-env','WAKEIT_ENROLL_TOKEN','--dir',runner_dir],dict(env,WAKEIT_ENROLL_TOKEN=enrollment))
        runner_config=json.loads((runner_dir/'runner.yaml').read_text()) if (runner_dir/'runner.yaml').read_text().lstrip().startswith('{') else None
        # Enrollment emits YAML; extract identity from the HTTP resource and write an equivalent JSON/YAML config.
        runner_row=api('/runners')[0]; runner_id=runner_row['id']
        cwd=root/'project with spaces 中文'; cwd.mkdir()
        remote=root/'external-jobs'; remote.mkdir()
        for id,mode in [('mock-sync','sync'),('mock-async','async')]:
            agent={'version':1,'id':id,'name':id,'logo':'/agent-logos/pi.svg' if mode=='sync' else None,'mode':mode,'submit':{'command':str(MOCK),'args':['sync' if mode=='sync' else 'submit'],'stdin':'{{task.prompt}}','env':{'WAKEIT_MOCK_DIR':str(remote)}},'detect':{'command':str(MOCK),'args':['--version']},'output':{'format':'text'},'poll_interval':'1s'}
            if mode=='async':
                agent['status']={'command':str(MOCK),'args':['status','{{job.id}}'],'env':{'WAKEIT_MOCK_DIR':str(remote)}}
                agent['cancel']={'command':str(MOCK),'args':['cancel','{{job.id}}'],'env':{'WAKEIT_MOCK_DIR':str(remote)}}
            (runner_dir/'agents'/f'{id}.yaml').write_text(json.dumps(agent))
        json_agent=json.loads((runner_dir/'agents/mock-sync.yaml').read_text());json_agent['id']='mock-sync-json';json_agent['name']='Mock JSON';json_agent['output']={'format':'json'}; (runner_dir/'agents/mock-sync-json.yaml').write_text(json.dumps(json_agent))
        for id in ['mock-sync','mock-sync-json','mock-async']:
            file=runner_dir/'agents'/f'{id}.yaml'; agent=json.loads(file.read_text())
            agent['parameters']={'model':{'type':'string','label':'模型','default':'fixture-default'},'reasoning':{'type':'string','label':'推理强度'}}
            if id!='mock-async':agent['models']={'command':str(MOCK),'args':['models'],'timeout':'1s'}
            file.write_text(json.dumps(agent))
        trace_agent=json.loads((runner_dir/'agents/mock-sync.yaml').read_text());trace_agent.update(id='mock-events',name='Mock Events',output={'format':'events'});trace_agent['submit']['args']=['events']
        (runner_dir/'agents/mock-events.yaml').write_text(json.dumps(trace_agent))
        from integration_sessions import configure as configure_sessions
        session_agent=configure_sessions(runner_dir,remote,MOCK)
        runner_config={'version':1,'gateway':base,'id':runner_id,'name':'test-device','token_file':str(runner_dir/'device.token'),'nats_url':config['nats_public_url'],'credentials_file':str(runner_dir/'runner.creds'),'data_dir':str(runner_dir/'data'),'concurrency':2,'agents':[str(runner_dir/'agents/mock-sync.yaml'),str(runner_dir/'agents/mock-async.yaml'),str(runner_dir/'agents/mock-sync-json.yaml'),str(runner_dir/'agents/mock-events.yaml'),str(session_agent)],'allowed_roots':[str(cwd)]}
        runner_file=runner_dir/'runner.yaml'; runner_file.write_text(json.dumps(runner_config))
        runner_process=spawn('runner',[BIN,'runner','start','--config',runner_file])
        wait(lambda:any(r['online'] and len(r['agents'])==5 for r in api('/runners')),'Runner registration and agent detection')
        check('Runner enrollment, WebSocket transport and local adapter detection')
        def task(id,agent='mock-sync',prompt='hello',schedule=None,**execution):
            return {'version':1,'id':id,'name':id,'runner':'test-device','agent':agent,'cwd':str(cwd),'prompt':prompt,'schedule':schedule,'enabled':True,'execution':dict(timeout='1h',start_grace='1h',overlap='skip',misfire='latest',**execution)}
        def apply(c,revision=None):return api('/tasks/'+c['id'],'PUT',{'config':c,'expected_revision':revision})
        def trigger(id,key=None):return api('/tasks/'+id+'/runs','POST',{},key)['id']
        def final(id,state='succeeded',timeout=25):return wait(lambda:(r if (r:=api('/runs/'+id))['state']==state else None), f'run {state}',timeout)
        from moments_checks import verify as verify_moments
        verify_moments(api,base,other,sql,task,apply,role_tokens,runner_id,check)
        from admin_archive_checks import verify as verify_admin_archive
        verify_admin_archive(api,base,other,sql,task,apply,trigger,final,role_tokens,runner_id,runner_dir,root,check)
        from integration_sessions import verify as verify_sessions
        verify_sessions(api,base,other,sql,task,apply,trigger,final,role_tokens,runner_id,runner_dir,root,check)
        release_meta=call(base,'/api/v1/meta')
        package_version=command([BIN,'--version']).strip().split()[-1]
        assert release_meta['version']==release_meta['install']['version']==package_version
        assert release_meta['install']['release'].endswith('/v'+package_version)
        assert release_meta['install']['manifest']==release_meta['install']['base_url']+'/wakeit-release.json'
        assert next(a for a in api('/agents') if a['id']=='mock-sync')['logo']=='/agent-logos/pi.svg'
        assert sql('SELECT count(*) FROM _sqlx_migrations')=='1'
        roots=api(f'/runners/{runner_id}/directories'); assert roots['directories'][0]['path']==str(cwd)
        for name in ['child 中文','other child']: (cwd/name).mkdir()
        from urllib.parse import urlencode
        def dirs(base_url=base,path=cwd,cursor=None):return call(base_url,'/api/v1/runners/'+runner_id+'/directories?'+urlencode({'path':str(path),**({'cursor':cursor} if cursor else {})}),token=token)
        assert {d['name'] for d in dirs()['directories']}=={'child 中文','other child'}
        assert dirs(other)['path']==str(cwd)
        expect_status(lambda:dirs(path=root),400)
        (cwd/'outside-link').symlink_to(root,target_is_directory=True)
        assert 'outside-link' not in {d['name'] for d in dirs()['directories']}
        expect_status(lambda:dirs(path=cwd/'outside-link'),400)
        expect_status(lambda:call(base,f'/api/v1/runners/{runner_id}/directories',token=role_tokens['viewer']),403)
        assert dirs(path=cwd/'child 中文')['parent']==str(cwd)
        for i in range(205):(cwd/f'page-{i:03}').mkdir()
        page=dirs();assert len(page['directories'])==200 and page['next_cursor']
        assert len(dirs(cursor=page['next_cursor'])['directories'])==7
        check('native directory queries across Gateways, pagination and symlink boundaries')
        def models(agent='mock-sync',base_url=base):return call(base_url,f'/api/v1/runners/{runner_id}/agents/{agent}/models/query','POST',{'cwd':str(cwd)},token)
        assert len(models()['models'])==2 and len(models(base_url=other)['models'])==2
        expect_status(lambda:models('mock-async'),400)
        for state in ['fail','invalid','sleep']:
            (cwd/'.models-state').write_text(state);expect_status(models,400)
        (cwd/'.models-state').write_text('empty');assert models()['models']==[]
        (cwd/'.models-state').unlink()
        expect_status(lambda:call(base,f'/api/v1/runners/{runner_id}/agents/mock-sync/models/query','POST',{'cwd':str(cwd)},role_tokens['viewer']),403)
        check('models command contract, no interface, empty, failure, malformed output and timeout')
        parameter_task=task('parameter-snapshot');parameter_task['parameters']={'model':'model-b','reasoning':'high'}
        apply(parameter_task);parameter_run=final(trigger('parameter-snapshot'))
        assert parameter_run['execution_snapshot']['parameters']==parameter_task['parameters']
        assert parameter_run['execution_snapshot']['cwd']==str(cwd)
        default_parameter_run=task('parameter-default');apply(default_parameter_run)
        assert final(trigger('parameter-default'))['execution_snapshot']['parameters']['model']=='fixture-default'
        invalid_parameters=task('parameter-invalid');invalid_parameters['parameters']={'undeclared':'value'};expect_status(lambda:apply(invalid_parameters),400)
        check('task parameter validation, adapter defaults and durable resolved snapshots')
        assert next(a for a in api('/agents') if a['id']=='mock-events')['trace_supported']
        assert not next(a for a in api('/agents') if a['id']=='mock-sync')['trace_supported']
        for native in ['codex','claude-code','pi','opencode']:
            native_args=[BIN,'adapter','stream',native,'--',MOCK,'native',ROOT/'crates/wakeit/tests/fixtures/trace'/f'{native}.jsonl']
            records=[json.loads(line) for line in command(native_args).splitlines()]
            assert any(r['type']=='result' and r['data']=='# 完成\n\n报告' for r in records)
            failure=subprocess.run([str(a) for a in native_args]+['fail'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            assert failure.returncode==7
        check('four native stream helpers preserve public events and native exit codes')
        apply(task('trace-live',agent='mock-events',prompt=json.dumps({'sleep':2,'text':'# 执行报告\n\n**完成**','count':5501,'corrupt':True})))
        trace_id=trigger('trace-live')
        wait(lambda:any(e['kind']=='trace' for e in api('/runs/'+trace_id+'/events?kind=trace')), 'live trace before exit')
        assert api('/runs/'+trace_id)['state']=='running'
        trace_run=final(trace_id,timeout=90)
        assert trace_run['result']=='# 执行报告\n\n**完成**' and trace_run['execution_snapshot']['trace_supported'] is True
        wait(lambda:len(api('/runs/'+trace_id+'/events?kind=trace'))>=5,'all trace snapshots persisted',timeout=90)
        trace_rows=api('/runs/'+trace_id+'/events?kind=trace')
        assert all(r['kind'] in ['trace','trace_status'] for r in trace_rows)
        assert next(r for r in trace_rows if r['kind']=='trace' and r['data']['id']=='cmd-1' and r['data']['status']=='running')['data']['occurred_at'] == next(r for r in trace_rows if r['kind']=='trace' and r['data']['id']=='cmd-1' and r['data']['status']=='succeeded')['data']['started_at']
        assert call(other,'/api/v1/runs/'+trace_id+'/events?kind=trace',token=token)==trace_rows
        assert call(base,'/api/v1/runs/'+trace_id+'/events?kind=trace',token=role_tokens['viewer'])==trace_rows
        cursor=0;total=0
        while True:
            rows=api('/runs/'+trace_id+'/events?after='+str(cursor));total+=len(rows)
            if len(rows)<500:break
            cursor=rows[-1]['id']
        assert total>5500
        check('live third-party trace, shared events, cursor paging over 5000 logs and independent Markdown result')
        apply(task('trace-cancel',agent='mock-events',prompt=json.dumps({'sleep':30})))
        cancel_trace=trigger('trace-cancel');wait(lambda:api('/runs/'+cancel_trace)['state']=='running','event process running')
        api('/runs/'+cancel_trace+'/cancel','POST',{});final(cancel_trace,'cancelled')
        assert (remote/'calls.log').read_text().count('events '+cancel_trace)==1
        check('event output leaves cancellation and process ownership unchanged')

        defaults=api('/settings'); api('/settings','PUT',dict(defaults,timezone='UTC'))
        default_task=apply(task('workspace-timezone',schedule={'cron':'0 9 * * *'}))
        assert default_task['config']['schedule']['timezone']=='UTC'
        preview=api('/schedule/preview','POST',{'cron':'0 9 * * *'})
        assert all(datetime.datetime.fromisoformat(t.replace('Z','+00:00')).hour==9 for t in preview)
        api('/settings','PUT',defaults)
        expect_status(lambda:call(base,'/api/v1/session','POST',{'username':42,'password':'invalid'}),422)
        check('workspace timezone defaults and structured input errors')
        c=task('literal',prompt="中文\n\"quotes\" ' $(touch SHOULD_NOT_EXIST) ; & {{job.id}}")
        saved=apply(c)
        expect_status(lambda:apply(c,0),409)
        expect_status(lambda:call(base,'/api/v1/tasks/literal/runs','POST',{},role_tokens['viewer']),403)
        run_id=trigger('literal','same-request')
        assert trigger('literal','same-request')==run_id
        result=final(run_id); assert c['prompt'] in result['result']; assert not (cwd/'SHOULD_NOT_EXIST').exists()
        check('optimistic revisions, idempotent trigger and literal Unicode/stdin handling')
        for i in range(23):
            fixture=task(f'pagination-fixture-{i:02}')
            fixture['name']=f'Pagination 中文 {i:02}';fixture['enabled']=False
            fixture['prompt']='literal %_ query';apply(fixture)
        query='?page=1&page_size=20&q=Pagination&enabled=false&runner=test-device&agent=mock-sync'
        first_page=api('/tasks'+query);second_page=api('/tasks'+query.replace('page=1','page=2'))
        assert first_page['total']==23 and len(first_page['items'])==20 and len(second_page['items'])==3
        assert not {t['id'] for t in first_page['items']} & {t['id'] for t in second_page['items']}
        assert call(other,'/api/v1/tasks'+query,token=token)==first_page
        assert api('/tasks?page=1&q='+urlencode({'q':'%_'})[2:])['total']==23
        assert api('/tasks?page=1&q=Pagination&enabled=true')['total']==0
        expect_status(lambda:api('/tasks?page=0'),400);expect_status(lambda:api('/runs?page=1&page_size=101'),400)
        sql("INSERT INTO runs(id,task_id,task_revision,runner_id,config,state,source,scheduled_at,deadline,finished_at) SELECT gen_random_uuid(),id,revision,runner_id,config,'succeeded','manual',now(),now()+interval '1 hour',now() FROM tasks WHERE id LIKE 'pagination-fixture-%'")
        first_page=api('/runs?page=1&page_size=20&q=Pagination&state=succeeded')
        second_page=api('/runs?page=2&page_size=20&q=Pagination&state=succeeded')
        assert first_page['total']==23 and len(first_page['items'])==20 and len(second_page['items'])==3
        assert not {r['id'] for r in first_page['items']} & {r['id'] for r in second_page['items']}
        assert api('/runs?page=1&q=Pagination&state=failed')['total']==0
        assert call(base,'/api/v1/runs?page=1&q=Pagination',token=role_tokens['viewer'])['total']==23
        check('server-side Task and Run pagination, literal fuzzy search, filters and cross-Gateway reads')
        assignment={'version':1,'run_id':run_id,'runner_id':runner_id,'task':c,'deadline':result['deadline']}
        replay=root/'replay.json';replay.write_text(json.dumps(assignment))
        command([PROBE,'replay',config['nats_url'],auth_dir/'gateway.creds',runner_id,replay])
        time.sleep(2)
        assert (remote/'calls.log').read_text().count('sync '+run_id)==1
        check('duplicate delivery does not restart an executed Run')
        sql(f"UPDATE runs SET finished_at=now()-interval '6 minutes' WHERE id='{run_id}'")
        wait(lambda:int(sql(f"SELECT count(*) FROM outbox WHERE payload->>'run_id'='{run_id}' AND cleared_at IS NULL"))==0,'cleanup of already acknowledged work')
        sql(f"UPDATE runs SET finished_at='{result['finished_at']}' WHERE id='{run_id}'")
        check('already acknowledged messages can be cleaned without repeated broker errors')
        for subject in ['$JS.API.STREAM.DELETE.WAKEIT_WORK',f'$JS.API.CONSUMER.INFO.WAKEIT_WORK.r_{uuid.uuid4().hex}_work',f'wakeit.v1.runner.{uuid.uuid4()}.events']:
            command([PROBE,'forbidden',config['nats_url'],runner_dir/'runner.creds',runner_id,subject])
        command([PROBE,'forbidden',config['nats_url'],runner_dir/'runner.creds',runner_id,'wakeit.v1.runner.'+str(uuid.uuid4())+'.query'])
        command([PROBE,'forbidden',config['nats_url'],runner_dir/'runner.creds',runner_id,'wakeit.v1.runner.'+str(uuid.uuid4())+'.reply.test'])
        check('NATS API and subject isolation for device credentials')
        apply(task('overlap',prompt=json.dumps({'sleep':2,'text':'overlap'})))
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
            results=list(executor.map(lambda _:trigger('overlap','parallel-idempotent'),range(8)))
        assert len(set(results))==1
        expect_status(lambda:trigger('overlap','different-request'),409)
        final(results[0]); check('concurrent manual triggers and overlap protection')
        at=(datetime.datetime.now(datetime.timezone.utc)+datetime.timedelta(seconds=2)).isoformat()
        apply(task('scheduled-once',schedule={'at':at,'timezone':'UTC'}))
        scheduled=wait(lambda:api('/runs?task=scheduled-once'),'scheduled Run')
        final(scheduled[0]['id']); assert len(api('/runs?task=scheduled-once'))==1
        check('two Gateway schedulers create exactly one occurrence')
        apply(task('async',agent='mock-async',prompt=json.dumps({'sleep':1,'text':'async result'})))
        async_id=trigger('async'); assert final(async_id)['result']=='async result'
        apply(task('cancel-async',agent='mock-async',prompt=json.dumps({'sleep':30})))
        cancel_id=trigger('cancel-async');wait(lambda:api('/runs/'+cancel_id)['job_id'],'external job id')
        api('/runs/'+cancel_id+'/cancel','POST',{}); final(cancel_id,'cancelled')
        check('async submit/status and confirmed cancellation')
        apply(task('cancel-sync',prompt=json.dumps({'sleep':30})))
        cancel_id=trigger('cancel-sync');wait(lambda:api('/runs/'+cancel_id)['state']=='running','sync starts')
        api('/runs/'+cancel_id+'/cancel','POST',{});final(cancel_id,'cancelled')
        check('sync cancellation terminates command process')
        apply(task('cancel-sync-json',agent='mock-sync-json',prompt=json.dumps({'sleep':30})))
        json_id=trigger('cancel-sync-json');wait(lambda:api('/runs/'+json_id)['state']=='running','JSON adapter starts')
        api('/runs/'+json_id+'/cancel','POST',{});final(json_id,'cancelled')
        check('JSON output interrupted by cancellation keeps the confirmed cancellation state')
        apply(task('restart-async',agent='mock-async',prompt=json.dumps({'sleep':8,'text':'resumed'})))
        restart_id=trigger('restart-async');wait(lambda:api('/runs/'+restart_id)['job_id'],'async durable job id')
        runner_process.kill();runner_process.wait()
        sql(f"UPDATE runners SET last_seen=now()-interval '61 seconds' WHERE id='{runner_id}'")
        runner_process=spawn('runner-restarted',[BIN,'runner','start','--config',runner_file])
        wait(lambda:api('/runners')[0]['online'],'Runner reconnect')
        command([PROBE,'short-ack',config['nats_url'],auth_dir/'gateway.creds',runner_id])
        assert final(restart_id,timeout=35)['result']=='resumed'
        assert (remote/'calls.log').read_text().count('submit '+restart_id)==1
        check('async recovery after SIGKILL without resubmitting')
        apply(task('uncertain',agent='mock-async',prompt=json.dumps({'sleep':.1,'uncertain':True})))
        uncertain_id=trigger('uncertain');final(uncertain_id,'unknown')
        time.sleep(2);assert (remote/'calls.log').read_text().count('submit '+uncertain_id)==1
        expect_status(lambda:api('/runs/'+uncertain_id+'/retry','POST',{}),409)
        api('/runs/'+uncertain_id+'/resolve','POST',{'state':'interrupted','reason':'verified deterministic fixture completion'})
        check('uncertain submit is not retried and requires explicit resolution')
        # Offline device: advertised adapter, but no consumer is connected.
        enroll_token=api('/runners/enrollment','POST',{})['token']; offline_dir=root/'offline'
        command([BIN,'--gateway',base,'runner','enroll','--name','offline-device','--token-env','WAKEIT_ENROLL_TOKEN','--dir',offline_dir],dict(env,WAKEIT_ENROLL_TOKEN=enroll_token))
        offline=[r for r in api('/runners') if r['name']=='offline-device'][0]
        offline_runtime=str(uuid.uuid4())
        call(base,'/api/v1/device/hello','POST',{'runtime_id':offline_runtime,'agents':[{'id':'mock-sync','name':'mock','mode':'sync','digest':'fixture','available':True,'version':'0.1','cancellable':True}],'info':{},'concurrency':2},(offline_dir/'device.token').read_text())
        c=task('latest',schedule={'every':'1s','timezone':'UTC'});c['runner']='offline-device'; saved=apply(c)
        time.sleep(3.5); pending=api('/runs?task=latest');assert sum(r['state']=='queued' for r in pending)==1
        c['enabled']=False;apply(c,saved['revision']);assert not any(r['state']=='queued' for r in api('/runs?task=latest'))
        check('latest missed occurrence coalescing and pause revocation')
        wait(lambda:int(sql(f"SELECT count(*) FROM outbox WHERE payload->>'run_id' IN (SELECT id::text FROM runs WHERE task_id='latest') AND cleared_at IS NULL"))==0,'obsolete queued messages are cleared')
        check('paused and superseded work is removed from JetStream')
        skip_at=(datetime.datetime.now(datetime.timezone.utc)+datetime.timedelta(seconds=1)).isoformat()
        c=task('offline-skip',schedule={'at':skip_at,'timezone':'UTC'});c['runner']='offline-device';c['execution']['misfire']='skip';apply(c)
        skipped=wait(lambda:(r if (r:=api('/runs?task=offline-skip')) and r[0]['state']=='skipped' else None),'offline misfire skip',timeout=12)
        assert skipped[0]['reason']=='misfire'
        check('skip misfire does not execute stale offline work')
        c=task('event-order');c['runner']='offline-device';apply(c)
        event_run=trigger('event-order'); event_claim=str(uuid.uuid4())
        claim=call(base,'/api/v1/device/claim','POST',{'runtime_id':offline_runtime,'run_id':event_run,'claim_id':event_claim,'agent_digest':'fixture'},(offline_dir/'device.token').read_text())
        assert claim['execute']
        def send_event(seq,state):
            event={'version':1,'runner_id':offline['id'],'run_id':event_run,'claim_id':event_claim,'seq':seq,'kind':'state','data':{'state':state},'timestamp':datetime.datetime.now(datetime.timezone.utc).isoformat()}
            file=root/'state-event.json';file.write_text(json.dumps(event))
            command([PROBE,'event',config['nats_url'],auth_dir/'gateway.creds',offline['id'],file])
        send_event(2,'unknown');final(event_run,'unknown')
        send_event(1,'running');send_event(2,'unknown')
        wait(lambda:len(api('/runs/'+event_run+'/events'))==2,'event deduplication')
        assert api('/runs/'+event_run)['state']=='unknown'
        send_event(3,'succeeded');final(event_run)
        send_event(4,'running');wait(lambda:len(api('/runs/'+event_run+'/events'))==4,'late event persisted')
        assert api('/runs/'+event_run)['state']=='succeeded'
        check('event sequence deduplication and terminal-state protection')
        occurrence='2026-10-01T01:00:00Z'
        def trace_event(seq,status='succeeded'):
            event={'version':1,'runner_id':offline['id'],'run_id':event_run,'claim_id':event_claim,'seq':seq,'kind':'trace','data':{'id':'durable-node','kind':'command','status':status,'title':'fixture command','input':'pwd','output':'完成','occurred_at':occurrence},'timestamp':occurrence}
            file=root/'trace-event.json';file.write_text(json.dumps(event));command([PROBE,'event',config['nats_url'],auth_dir/'gateway.creds',offline['id'],file])
        trace_event(5);trace_event(5)
        wait(lambda:len(api('/runs/'+event_run+'/events?kind=trace'))==1,'trace sequence deduplication')
        row=api('/runs/'+event_run+'/events?kind=trace')[0]
        assert row['data']['occurred_at']==occurrence and row['data']['event_time'].startswith('2026-10-01T01:00:00')
        assert not row['created_at'].startswith('2026-10-01')
        trace_event(6,'running');wait(lambda:len(api('/runs/'+event_run+'/events?kind=trace'))==2,'late trace')
        assert api('/runs/'+event_run)['state']=='succeeded'
        sql(f"UPDATE runs SET log_bytes=20971519 WHERE id='{event_run}'")
        trace_event(7)
        wait(lambda:any(e['kind']=='trace_status' and e['data']['status']=='truncated' for e in api('/runs/'+event_run+'/events?kind=trace')),'shared content cap')
        assert not any(e['kind']=='trace' and e['seq']==7 for e in api('/runs/'+event_run+'/events?kind=trace'))
        sql(f"UPDATE events SET created_at=now()-interval '8 days' WHERE run_id='{event_run}' AND kind='trace'")
        wait(lambda:any(e['kind']=='trace_status' and e['data']['status']=='expired' for e in api('/runs/'+event_run+'/events?kind=trace')),'trace retention marker',timeout=40)
        assert not any(e['kind']=='trace' for e in api('/runs/'+event_run+'/events?kind=trace'))
        assert api('/runs/'+event_run)['state']=='succeeded'
        malformed={'version':1,'runner_id':offline['id'],'run_id':event_run,'claim_id':event_claim,'seq':8,'kind':'trace','data':None,'timestamp':occurrence}
        file=root/'malformed-trace-event.json';file.write_text(json.dumps(malformed));command([PROBE,'event',config['nats_url'],auth_dir/'gateway.creds',offline['id'],file])
        wait(lambda:any(e['seq']==8 and e['kind']=='trace_status' and e['data']['status']=='incomplete' for e in api('/runs/'+event_run+'/events?kind=trace')),'malformed transport data does not stop the consumer')
        trace_event(9)
        check('trace replay deduplication, preserved occurrence times, shared content limit and retained expiry markers')

        apply(task('broker-recovery',prompt='broker recovered'))
        stop(broker); recovery_id=trigger('broker-recovery')
        wait(lambda:int(sql("SELECT count(*) FROM outbox WHERE published_at IS NULL"))>0,'transactional outbox retains task')
        broker=spawn('nats-restarted',[shutil.which('nats-server'),'--config',auth_dir/'nats.conf'])
        final(recovery_id,timeout=35);check('broker restart preserves outbox and durable consumers')
        apply(task('gateway-recovery',prompt=json.dumps({'sleep':3,'text':'gateway recovered'})))
        gateway_id=trigger('gateway-recovery');wait(lambda:api('/runs/'+gateway_id)['state']=='running','run started before Gateway outage')
        first.kill();first.wait();second.kill();second.wait()
        time.sleep(4)
        first=spawn('gateway-recovered',[BIN,'gateway','start','--config',config_file]);second=spawn('gateway-second-recovered',[BIN,'gateway','start','--config',second_config])
        wait(lambda:call(base,'/health/ready'),'Gateway restored')
        assert 'gateway recovered' in final(gateway_id)['result']
        assert (remote/'calls.log').read_text().count('sync '+gateway_id)==1
        check('Gateway crash preserves the running process and its durable result')
        # An SSE cursor can be resumed through either Gateway because events live in PostgreSQL.
        events=api('/runs/'+run_id+'/events'); assert events
        resumed=call(other,f'/api/v1/runs/{run_id}/events?after={events[0]["id"]}',token=token)
        assert all(event['id']>events[0]['id'] for event in resumed)
        req=urllib.request.Request(other+f'/api/v1/runs/{run_id}/stream',headers={'Authorization':'Bearer '+token})
        with urllib.request.urlopen(req,timeout=5) as response:
            assert response.headers['Content-Type'].startswith('text/event-stream')
            assert any(b'run_event' in response.readline() for _ in range(6))
        check('SSE and cursor reads across Gateway instances')
        api('/runners/'+offline['id']+'/revoke','POST',{})
        expect_status(lambda:call(base,'/api/v1/device/heartbeat','POST',{'runtime_id':str(uuid.uuid4()),'claims':[],'info':{}},(offline_dir/'device.token').read_text()),401)
        expect_status(lambda:trigger('latest'),409)
        assert not api('/tasks/latest')['config']['enabled']
        expect_status(lambda:api('/runners/'+offline['id']+'/directories'),404)
        expect_status(lambda:api('/runners/'+offline['id']+'/agents/mock-sync/models/query','POST',{'cwd':str(cwd)}),404)
        check('device revocation rejects subsequent API access and execution')
        from integration_sessions import verify_recovered_commands
        def restart_for_command_recovery():
            nonlocal runner_process
            stop(runner_process)
            sql(f"UPDATE runners SET last_seen=now()-interval '61 seconds' WHERE id='{runner_id}'")
            runner_process=spawn('runner-command-recovery',[BIN,'runner','start','--config',runner_file])
        verify_recovered_commands(api,sql,runner_dir,remote,restart_for_command_recovery,check)
        from device_auth_checks import verify as verify_device_auth
        verify_device_auth(api,base,other,sql,role_tokens,check)
        state={'base_url':base,'other_url':other,'username':'admin','password':env['WAKEIT_PASSWORD'],'stack_pid':os.getpid(),'pids':[p.pid for p in processes if p.poll() is None],'pg_bin':str(pg_bin),'pg_data':str(root/'pg'),'artifact_dir':str(root),'checks':checks,'runner_id':runner_id,'runner_config':str(runner_file)}
        state_file=ROOT/args.state_file;state_file.parent.mkdir(parents=True,exist_ok=True);state_file.write_text(json.dumps(state,indent=2));state_file.chmod(0o600)
        success=True;print(f'{len(checks)} integration scenarios passed. Artifacts: {root.relative_to(ROOT)}',flush=True)
        if args.keep_running:
            print(f'Test stack remains available at {base}; press Ctrl+C to stop.',flush=True)
            def interrupt(signum,frame):raise KeyboardInterrupt
            signal.signal(signal.SIGTERM,interrupt)
            try:
                while True:time.sleep(1)
            except KeyboardInterrupt:pass
    finally:
        for process in reversed(processes):stop(process)
        if pg_started:subprocess.run([str(pg_bin/'pg_ctl'),'-D',str(root/'pg'),'-m','fast','stop'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        for file in files:file.close()
        if not success:print(f'Integration logs preserved at {root}',flush=True)

if __name__=='__main__':main()
