// Synthetic Kimi Server API fixture. No provider calls or user data.
const http=require('node:http'),fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
const home=process.env.KIMI_CODE_HOME,mode=process.env.WAKEIT_KIMI_FIXTURE||'ok';
const sessionId='session_fixture-live';const token='synthetic-fixture-only';let websocket,seq=0,warmed=false;
const traceFile=path.join(home,'requests.jsonl');
const send=(type,payload)=>{if(!websocket)return;const data=Buffer.from(JSON.stringify({type,seq:++seq,session_id:sessionId,timestamp:new Date().toISOString(),payload}));let head;if(data.length<126)head=Buffer.from([129,data.length]);else{head=Buffer.alloc(4);head[0]=129;head[1]=126;head.writeUInt16BE(data.length,2)}websocket.write(Buffer.concat([head,data]));};
function info(){const meta=JSON.parse(fs.readFileSync(path.join(home,'state.json')));return {id:sessionId,title:meta.title,metadata:{cwd:meta.cwd}}}
const server=http.createServer(async(req,res)=>{
 let raw='';for await(const chunk of req)raw+=chunk;const body=raw?JSON.parse(raw):null;
 fs.appendFileSync(traceFile,JSON.stringify({method:req.method,path:req.url,body})+'\n');
 if(req.headers.authorization!==`Bearer ${token}`){res.writeHead(401);res.end('{}');return}
 const ok=data=>{res.setHeader('Content-Type','application/json');res.end(JSON.stringify({code:0,data}))};
 if(req.url==='/api/v1/meta')return ok({server_id:'api-id-independent-from-registry',dangerous_bypass_auth:false,server_version:'2.fixture'});
 if(req.url==='/api/v1/config')return ok({default_model:'fixture-model'});
 if(req.url==='/api/v1/sessions'&&req.method==='POST'){
  const cwd=mode==='wrong-cwd'?home:body.metadata.cwd;const title=mode==='broken-title'?'':body.title;
  fs.writeFileSync(path.join(home,'state.json'),JSON.stringify({id:sessionId,cwd,title}));
  fs.writeFileSync(path.join(home,'session_index.jsonl'),JSON.stringify({sessionId,workDir:cwd,sessionDir:home})+'\n');return ok(info());
 }
 if(req.url===`/api/v1/sessions/${sessionId}`)return ok(info());
 if(req.url===`/api/v1/sessions/${sessionId}/profile`){warmed=true;return ok(info());}
 if(req.url===`/api/v1/sessions/${sessionId}/prompts`){
  if(mode==='reject'){res.writeHead(400);res.end(JSON.stringify({code:40001}));return}
  ok({prompt_id:'fixture-prompt',status:'running'});if(mode==='cold'&&!warmed)return;
  send('turn.started',{agentId:'main',turnId:0});send('turn.step.started',{agentId:'main',turnId:0});
  send('assistant.delta',{agentId:'main',turnId:0,delta:'Before '});send('assistant.delta',{agentId:'main',turnId:0,delta:'tool 中文'});
  if(mode==='disconnect'){websocket.destroy();return}
  send('tool.call.started',{agentId:'main',turnId:0,toolCallId:'call1',name:'Bash',args:{command:'printf fixture'}});
  setTimeout(()=>{
   fs.writeFileSync(path.join(home,'tool-completed'),'done');
   send('tool.result',{agentId:'main',turnId:0,toolCallId:'call1',output:{text:'fixture tool output'}});
   send('turn.step.started',{agentId:'main',turnId:0});send('assistant.delta',{agentId:'main',turnId:0,delta:'Final report'});
   send('turn.ended',{agentId:'main',turnId:0,reason:mode==='fail'?'failed':'completed',error:mode==='fail'?{code:'fixture.failure'}:undefined});
  },300);return;
 }
 if(req.url.endsWith(':abort'))return ok({});
 if(req.url==='/api/v1/shutdown'){ok({});setTimeout(()=>process.exit(mode==='shutdown-fail'?7:0),20);return}
 res.writeHead(404);res.end(JSON.stringify({code:40401}));
});
server.on('upgrade',(req,socket)=>{
 if(req.headers.authorization!==`Bearer ${token}`){socket.destroy();return}
 const accept=crypto.createHash('sha1').update(req.headers['sec-websocket-key']+'258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
 socket.write('HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: '+accept+'\r\n\r\n');websocket=socket;
 // Only the small masked subscribe frame needs decoding in this fixture.
 let buffer=Buffer.alloc(0);socket.on('data',chunk=>{buffer=Buffer.concat([buffer,chunk]);if(buffer.length<2)return;if((buffer[0]&15)===8){socket.end(Buffer.from([136,0]));buffer=Buffer.alloc(0);return}let len=buffer[1]&127,offset=2;if(len===126){if(buffer.length<4)return;len=buffer.readUInt16BE(2);offset=4}if(buffer.length<offset+4+len)return;const mask=buffer.subarray(offset,offset+4),data=buffer.subarray(offset+4,offset+4+len);for(let i=0;i<data.length;i++)data[i]^=mask[i%4];buffer=buffer.subarray(offset+4+len);try{const v=JSON.parse(data);if(v.type==='pong'){fs.writeFileSync(path.join(home,'pong-received'),'yes');return}const payload=JSON.stringify({type:'ack',id:v.id,code:0,payload:{accepted:[sessionId]}});socket.write(Buffer.concat([Buffer.from([129,Buffer.byteLength(payload)]),Buffer.from(payload)]));setTimeout(()=>send('ping',{nonce:'fixture-nonce'}),50);}catch{}});
});
const args=process.argv.slice(2),port=Number(args[args.indexOf('--port')+1]);
server.listen(port,'127.0.0.1',()=>{fs.mkdirSync(path.join(home,'server/instances'),{recursive:true});fs.writeFileSync(path.join(home,'server.token'),token,{mode:0o600});fs.writeFileSync(path.join(home,'server/instances/fixture.json'),JSON.stringify({pid:process.pid,host:'127.0.0.1',port,server_id:'registry-id'}));process.stdout.write('Bearer banner: '+token+'\n');});
