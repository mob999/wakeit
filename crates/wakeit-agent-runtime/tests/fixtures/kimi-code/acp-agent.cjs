// Synthetic ACP peer. No model service, credentials, or real user sessions.
const fs = require('node:fs');
const path = require('node:path');
const readline = require('node:readline');
const mode = process.argv[2] || 'normal';
const home = process.env.KIMI_CODE_HOME;
const directory = path.join(home, 'fixture-session');
const stateFile = path.join(directory, 'state.json');
const id = 'kimi-acp-fixture';
let active;
let configured = false;
const write = (message) => process.stdout.write(JSON.stringify({jsonrpc:'2.0', ...message})+'\n');
const reply = (request, result) => write({id:request.id, result});
const error = (request, message) => write({id:request.id,error:{code:-32602,message}});
const update = (data) => write({method:'session/update',params:{sessionId:mode==='wrong-session'?'another-session':id,update:data}});
const event = (sessionUpdate, data={}) => update({sessionUpdate,...data});
function metadata() {return JSON.parse(fs.readFileSync(stateFile,'utf8'));}
function store(value) {fs.writeFileSync(stateFile,JSON.stringify(value));}
function start(cwd) {
    fs.mkdirSync(directory,{recursive:true});
    store({id,cwd,title:null,model:'saved-model',updatedAt:1});
    fs.appendFileSync(path.join(home,'session_index.jsonl'),JSON.stringify({sessionId:id,workDir:cwd,sessionDir:directory})+'\n');
}
function finish(request) {
    if (mode==='disconnect') {process.exit(0);}
    if (mode==='rpc-error') {error(request,'synthetic prompt failure');return;}
    if (mode==='incomplete') {reply(request,{stopReason:'end_turn'});return;}
    event('tool_call_update',{toolCallId:'tool-one',status:'completed',rawOutput:'ACP_TOOL_OUTPUT'});
    event('agent_message_chunk',{content:{type:'text',text:'ACP_REPORT'}});
    reply(request,{stopReason:mode==='failed-stop'?'max_tokens':'end_turn'});
}
readline.createInterface({input:process.stdin}).on('line',line=>{
    const request=JSON.parse(line),p=request.params||{};
    if (!request.method) {
        fs.writeFileSync(path.join(home,'permission.json'),JSON.stringify(request.result));
        finish(active);return;
    }
    fs.appendFileSync(path.join(home,'calls.jsonl'),JSON.stringify({method:request.method,params:p})+'\n');
    switch(request.method) {
        case 'initialize':reply(request,{protocolVersion:1,agentInfo:{name:'fixture',version:'2.4.1'},agentCapabilities:{loadSession:true,sessionCapabilities:{resume:{}}}});break;
        case 'session/new':start(p.cwd);reply(request,{sessionId:id,configOptions:[]});break;
        case 'session/resume':
            if (!fs.existsSync(stateFile) || p.sessionId!==id) {error(request,'missing session');break;}
            reply(request,{configOptions:[]});break;
        case 'session/set_config_option':
            if (p.value==='invalid') {error(request,'unknown model');break;}
            if(p.configId==='model') {store({...metadata(),model:p.value});}
            reply(request,{configOptions:[]});break;
        case 'session/set_mode':configured=p.modeId==='auto';reply(request,{});break;
        case 'session/prompt':
            if (!configured) {error(request,'mode was not configured');break;}
            active=request;
            event('agent_thought_chunk',{content:{type:'text',text:'PRIVATE_REASONING_MUST_NOT_LEAK'}});
            event('agent_message_chunk',{content:{type:'text',text:'Starting'}});
            event('tool_call',{toolCallId:'tool-one',title:'Fixture tool',kind:'execute',status:'pending'});
            if(mode==='permission') {write({id:900,method:'session/request_permission',params:{sessionId:id,options:[{kind:'allow_once',optionId:'yes'},{kind:'reject_once',optionId:'no'}]}});break;}
            if(mode==='wait') break;
            setTimeout(()=>finish(request),200);break;
        case 'session/cancel':reply(active,{stopReason:'cancelled'});break;
        case 'session/close':reply(request,{});break;
        default:error(request,'unsupported method');
    }
}).on('close',()=>process.exit(mode==='shutdown-failure'?9:0));
