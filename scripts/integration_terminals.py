#!/usr/bin/env python3
"""Disposable PostgreSQL/NATS/two-Gateway/PTY acceptance; no host connections changed."""
import base64
import getpass
import hashlib
import json
import os
import pathlib
import shutil
import signal
import socket
import sqlite3
import struct
import subprocess
import time
import uuid
from integration import call, command, expect_status, port, wait
from integration_sessions import configure

ROOT = pathlib.Path(__file__).resolve().parents[1]
BIN = ROOT / 'target/debug/wakeit'
GATEWAY = ROOT / 'target/debug/wakeit-gateway'
MOCK = ROOT / 'target/debug/examples/mock_agent'

class WS:
    """Small RFC6455 text client for the loopback acceptance fixture (stdlib only)."""
    def __init__(self, base, path, token, origin=None):
        from urllib.parse import urlsplit
        url = urlsplit(base)
        self.socket = socket.create_connection((url.hostname, url.port), 5)
        self.buffer = b''
        key = base64.b64encode(os.urandom(16)).decode()
        self.socket.sendall((f'GET {path} HTTP/1.1\r\nHost: {url.netloc}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\nOrigin: {origin or base}\r\nAuthorization: Bearer {token}\r\n\r\n').encode())
        while b'\r\n\r\n' not in self.buffer: self.buffer += self.socket.recv(65536)
        head, self.buffer = self.buffer.split(b'\r\n\r\n', 1)
        assert head.startswith(b'HTTP/1.1 101'), head.split(b'\r\n')[0]
        expected = base64.b64encode(hashlib.sha1((key+'258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode()).digest())
        assert expected.lower() in head.lower()
    def read(self, count):
        while len(self.buffer) < count:
            data = self.socket.recv(65536)
            if not data: raise EOFError('WebSocket closed')
            self.buffer += data
        value, self.buffer = self.buffer[:count], self.buffer[count:]
        return value
    def send(self, value):
        payload = json.dumps(value).encode()
        mask = os.urandom(4)
        length = len(payload)
        header = bytes([0x81, 0x80 | length]) if length < 126 else bytes([0x81, 0x80 | 126])+struct.pack('!H', length)
        self.socket.sendall(header+mask+bytes(b ^ mask[i % 4] for i,b in enumerate(payload)))
    def recv(self, timeout=8):
        self.socket.settimeout(timeout)
        first, second = self.read(2)
        length = second & 127
        if length == 126: length = struct.unpack('!H', self.read(2))[0]
        if length == 127: length = struct.unpack('!Q', self.read(8))[0]
        mask = self.read(4) if second & 128 else None
        data = self.read(length)
        if mask: data = bytes(b ^ mask[i % 4] for i,b in enumerate(data))
        if first & 15 == 8: raise EOFError('WebSocket closed')
        return json.loads(data)
    def until(self, predicate, timeout=10):
        deadline = time.monotonic()+timeout
        while time.monotonic() < deadline:
            value = self.recv(max(.1, deadline-time.monotonic()))
            if predicate(value): return value
        raise AssertionError('terminal frame not received')
    def close(self): self.socket.close()


def main():
    root = ROOT / '.local' / ('terminals-'+uuid.uuid4().hex[:8])
    root.mkdir(parents=True)
    os.chmod(root, 0o700)
    pg = pathlib.Path(command(['pg_config','--bindir']).strip())
    ports = [port() for _ in range(5)]
    pgport,nats,wsport,http,second = ports
    base,other = f'http://127.0.0.1:{http}',f'http://127.0.0.1:{second}'
    env = dict(os.environ, HOME=str(root/'home'), XDG_CONFIG_HOME=str(root/'home'), WAKEIT_DATABASE_URL=f'postgres://{getpass.getuser()}@127.0.0.1:{pgport}/postgres', WAKEIT_PASSWORD='terminal-fixture-password')
    (root/'home').mkdir()
    processes,logs,checks = [],[],[]
    def spawn(name,args):
        log=open(root/(name+'.log'),'ab',buffering=0);logs.append(log)
        p=subprocess.Popen([str(a) for a in args],env=env,cwd=ROOT,stdout=log,stderr=log,start_new_session=True)
        processes.append(p);return p
    def passed(name): checks.append(name);print('PASS '+name,flush=True)
    started=False
    try:
        command([pg/'initdb','-D',root/'pg','-A','trust','--no-locale','-E','UTF8'])
        command([pg/'pg_ctl','-D',root/'pg','-l',root/'postgres.log','-o',f"-p {pgport} -h 127.0.0.1 -k ''",'start']);started=True
        auth=root/'nats'
        command([GATEWAY,'nats-init','--dir',auth,'--port',nats,'--ws-port',wsport],env)
        spawn('nats',[shutil.which('nats-server'),'--config',auth/'nats.conf'])
        config=dict(version=1,listen=f'127.0.0.1:{http}',public_url=base,database_url_env='WAKEIT_DATABASE_URL',nats_url=f'nats://127.0.0.1:{nats}',nats_public_url=f'ws://127.0.0.1:{wsport}',nats_auth_dir=str(auth))
        cfg=root/'gateway.yaml';cfg.write_text(json.dumps(config))
        cfg2=root/'gateway-second.yaml';cfg2.write_text(json.dumps(dict(config,listen=f'127.0.0.1:{second}')))
        command([GATEWAY,'migrate','--config',cfg],env)
        command([GATEWAY,'bootstrap','--config',cfg,'--username','admin'],env)
        g1=spawn('gateway',[GATEWAY,'start','--config',cfg]);g2=spawn('gateway-second',[GATEWAY,'start','--config',cfg2])
        wait(lambda:call(base,'/health/ready'),'gateway')
        token=call(base,'/api/v1/session','POST',dict(username='admin',password=env['WAKEIT_PASSWORD']))['token']
        global_api=lambda p,m='GET',b=None,t=token:call(base,'/api/v1'+p,m,b,t)
        workspace=global_api('/workspaces')['items'][0]['id']
        prefix='/api/v1/workspaces/'+workspace
        def api(path,method='GET',body=None,t=token,g=base):return call(g,prefix+path,method,body,t)
        for role in ['operator','viewer']:
            global_api('/instance/users','POST',dict(username=role,password=env['WAKEIT_PASSWORD'],role=role))
            invitation=api('/invites','POST',{'role':role})['token']
            credential=global_api('/session','POST',dict(username=role,password=env['WAKEIT_PASSWORD']))['token']
            global_api('/invites/accept','POST',{'token':invitation},credential)
            env[role]=credential
        enrollment=api('/runners/enrollment','POST',{})['token']
        directory=root/'runner'
        command([BIN,'--gateway',base,'runner','enroll','--name','terminal-device','--token-env','ENROLLMENT','--dir',directory],dict(env,ENROLLMENT=enrollment))
        runner=api('/runners')[0]['id']
        project=root/'project';project.mkdir()
        remote=root/'external-jobs';remote.mkdir()
        (directory/'agents/mock-events.yaml').write_text(json.dumps(dict(version=1,id='mock-events',name='Mock events',mode='sync',submit=dict(command=str(MOCK),args=['events'],stdin='{{task.prompt}}'),detect=dict(command=str(MOCK),args=['--version']),output=dict(format='events'))))
        adapter=configure(directory,remote,MOCK)
        interactive=root/'interactive.py'
        interactive.write_text('''import os,signal,sys,tty,subprocess\nchild=subprocess.Popen([sys.executable,'-c','import time;time.sleep(3600)'])\ndef stop(*args):\n child.terminate();child.wait();sys.exit(0)\nsignal.signal(signal.SIGTERM,stop)\nprint('fixture pid='+str(os.getpid())+' child='+str(child.pid),flush=True)\ntty.setraw(0)\nwhile True:\n data=os.read(0,16384)\n if not data: break\n if data==b'exit':stop()\n os.write(1,b'INPUT:'+data+b'\\r\\n')\n''')
        agent=json.loads(adapter.read_text())
        agent['submit']['env']={'WAKEIT_MOCK_DIR':str(remote)}
        agent['session']['interactive']={'command':shutil.which('python3'),'args':[str(interactive),'{{session.id}}']}
        adapter.write_text(json.dumps(agent))
        rcfg=directory/'runner.yaml'
        rcfg.write_text(json.dumps(dict(version=1,gateway=base,id=runner,name='terminal-device',token_file=str(directory/'device.token'),nats_url=config['nats_public_url'],credentials_file=str(directory/'runner.creds'),data_dir=str(directory/'data'),concurrency=2,agents=[str(adapter)],allowed_roots=[str(project)])))
        runner_process=spawn('runner' ,[BIN,'runner','start','--config',rcfg])
        wait(lambda:any(r['online'] and r['info'].get('web_terminal')==1 for r in api('/runners')),'terminal capability')
        definition=dict(version=1,id='terminal-task',name='Terminal fixture',runner='terminal-device',agent='mock-session',cwd=str(project),prompt=json.dumps({'text':'unchanged report'}),enabled=True,schedule=None)
        api('/tasks/terminal-task','PUT',{'config':definition,'expected_revision':None})
        run=api('/tasks/terminal-task/runs','POST',{})['id']
        original=wait(lambda:(r if (r:=api('/runs/'+run)).get('session') and r['session'].get('ready') and r['state']=='succeeded' else None),'resumable Run')
        session=original['session']['id']
        def create(request=None):return api('/runs/'+run+'/terminals','POST',{'request_id':request or str(uuid.uuid4())},t=env['operator'])
        def attach(record,g=base):
            ticket=api('/terminals/'+record['id']+'/attach','POST',{},t=env['operator'],g=g)['ticket']
            stream=WS(g,prefix+'/terminals/'+record['id']+'/stream',env['operator'],base)
            stream.send({'ticket':ticket,'cols':100,'rows':30})
            return stream
        dormant=create()
        ticket=api('/terminals/'+dormant['id']+'/attach','POST',{},t=env['operator'])['ticket']
        assert api('/terminals/'+dormant['id']+'/close','POST',{},t=env['operator'])['state']=='closed'
        late=WS(base,prefix+'/terminals/'+dormant['id']+'/stream',env['operator'])
        late.send({'ticket':ticket,'cols':100,'rows':30})
        try:late.recv(5)
        except EOFError:pass
        else:raise AssertionError('closed terminal accepted late attachment')
        late.close()
        expect_status(lambda:api('/terminals/'+dormant['id']+'/attach','POST',{},t=env['operator']),409)
        with sqlite3.connect(directory/'data/journal.sqlite') as db:assert db.execute('select count(*) from local_terminals').fetchone()[0]==0
        request=str(uuid.uuid4());record=create(request)
        assert create(request)['id']==record['id']
        with sqlite3.connect(directory/'data/journal.sqlite') as db:assert db.execute('select count(*) from local_terminals').fetchone()[0]==0
        expect_status(lambda:api('/runs/'+run+'/terminals','POST',{'request_id':str(uuid.uuid4())},t=env['viewer']),403)
        other_workspace=global_api('/workspaces','POST',{'name':'Other'})['id']
        expect_status(lambda:call(base,'/api/v1/workspaces/'+other_workspace+'/terminals/'+record['id'],token=token),404)
        passed('migration, Operator access, Viewer/cross-workspace protection, idempotency and no process before attachment')
        stream=attach(record,other)
        stream.until(lambda f:f['kind']=='state' and f['data']['state']=='running')
        output=stream.until(lambda f:f['kind']=='output')
        assert 'fixture pid=' in base64.b64decode(output['data']['data']).decode()
        expect_status(lambda:create(),409)
        expect_status(lambda:api('/terminals/'+record['id']+'/attach','POST',{},t=env['operator']),409)
        stream.send({'kind':'input','data':{'seq':1,'data':base64.b64encode('中文\t\x1b[A\x03'.encode()).decode()}})
        echoed=stream.until(lambda f:f['kind']=='output' and b'INPUT:' in base64.b64decode(f['data']['data']))
        assert '中文'.encode() in base64.b64decode(echoed['data']['data'])
        stream.send({'kind':'resize','data':{'cols':60,'rows':18}})
        stream.send({'kind':'snapshot','data':{}})
        assert stream.until(lambda f:f['kind']=='snapshot')['data']['screen']
        cli=subprocess.run([str(BIN),'run','resume',run,'--runner-config',str(rcfg)],env=env,text=True,capture_output=True,timeout=15)
        assert cli.returncode!=0 and 'use' in cli.stderr.lower()
        bound=dict(definition,id='bound',session={'id':session})
        api('/tasks/bound','PUT',{'config':bound,'expected_revision':None})
        blocked=api('/tasks/bound/runs','POST',{})['id']
        assert wait(lambda:(r if (r:=api('/runs/'+blocked))['state']=='skipped' else None),'bound session blocked')
        passed('real PTY, Unicode/control input, resize/screen sync, single writer, cross-Gateway routing and task/CLI Session exclusion')
        stream.close()
        wait(lambda:not create().get('state')=='closed','detached metadata')
        recovered=attach(record)
        snapshot=recovered.until(lambda f:f['kind']=='snapshot')
        assert b'fixture pid=' in base64.b64decode(snapshot['data']['screen'])
        with sqlite3.connect(directory/'data/journal.sqlite') as db:assert db.execute('select count(*) from local_terminals').fetchone()[0]==1
        closed=api('/terminals/'+record['id']+'/close','POST',{},t=env['operator'])
        assert closed['state']=='closed',closed
        recovered.close()
        assert api('/terminals/'+record['id']+'/close','POST',{},t=env['operator'])['state']=='closed'
        passed('disconnect reattaches existing process, no duplicate start, explicit close confirms process tree and is idempotent')
        final=api('/runs/'+run)
        for key in ['result','state','finished_at','session']:assert final[key]==original[key]
        expired=create();lost=attach(expired);lost.until(lambda f:f['kind']=='state' and f['data']['state']=='running');lost.close()
        time.sleep(32)
        resumed=api('/tasks/bound/runs','POST',{})['id']
        assert wait(lambda:(r if (r:=api('/runs/'+resumed))['state']=='succeeded' else None),'free Session after terminal lease')
        assert api('/terminals/'+expired['id'],t=env['operator'])['state']=='closed'
        passed('30-second local lease cleanup and immutable original Run/result/session')
        pending=create();connection=attach(pending);connection.until(lambda f:f['kind']=='state' and f['data']['state']=='running')
        operator=global_api('/session',t=env['operator'])['id']
        api('/members/'+operator,'PUT',{'role':'viewer','active':True})
        connection.send({'kind':'heartbeat'})
        time.sleep(6)
        # Admin cannot take another credential\'s terminal; inspect only synthetic metadata through SQL.
        state=command([pg/'psql',env['WAKEIT_DATABASE_URL'],'-Atc',f"SELECT state FROM terminals WHERE id='{pending['id']}'"])
        assert state.strip()=='closed',state
        connection.close()
        passed('membership downgrade closes active channel and process without waiting for user action')
        (root/'acceptance.json').write_text(json.dumps({'checks':checks,'success':True},indent=2))
        print('Acceptance saved in '+str(root/'acceptance.json'),flush=True)
    finally:
        for process in reversed(processes):
            if process.poll() is None:
                process.terminate()
                try:process.wait(timeout=15)
                except subprocess.TimeoutExpired:os.killpg(process.pid,signal.SIGKILL);process.wait()
        if started:subprocess.run([str(pg/'pg_ctl'),'-D',str(root/'pg'),'-m','fast','stop'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        for log in logs:log.close()

if __name__=='__main__':main()
