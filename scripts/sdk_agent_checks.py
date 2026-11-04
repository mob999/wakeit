"""SDK-only Agent acceptance on a disposable Gateway/NATS/Runner stack."""
import json
from integration_support import ROOT, BIN, command, expect_status, wait


def configure(root):
    """Generate the private synthetic package; never install it on a daily Runner."""
    package = root / 'sdk-agent-package'
    package.mkdir()
    descriptor = {
        'name': 'Synthetic SDK Agent', 'mode': 'sync',
        'operations': ['detect', 'execute', 'models', 'reconcile'],
        'parameters': {'model': {'type': 'string', 'default': 'synthetic-model'}},
    }
    async_descriptor = {
        'name': 'Synthetic asynchronous SDK Agent', 'mode': 'async',
        'operations': ['detect', 'submit', 'poll', 'cancel', 'reconcile'],
        'parameters': {},
    }
    manifest = {
        'manifest_version': 1, 'name': 'synthetic-sdk-only-agent', 'version': '0.0.0',
        'wakeit': {'protocol': 1, 'runtimes': ['node'], 'plugins': {'synthetic': {
            'name': 'Synthetic SDK', 'command': 'node', 'args': ['{{plugin.root}}/agent.ts'],
            'agents': {'sdk-agent': descriptor, 'sdk-async': async_descriptor},
        }}},
    }
    (package / 'wakeit-plugin.json').write_text(json.dumps(manifest))
    sdk = (ROOT / 'sdk/typescript/src/index.ts').as_uri()
    source = """
import { Agent, Plugin } from SDK_MODULE;
import fs from 'node:fs/promises';
import path from 'node:path';
const descriptor = DESCRIPTOR;
const implementation = new Agent(descriptor)
 .on('detect', () => ({kind:'detection',value:{available:true,version:'synthetic',reason:null}}))
 .on('models', () => ({kind:'models',value:{version:1,models:[{id:'synthetic-model',name:'Synthetic model',parameters:{model:'synthetic-model'},parameter_options:{}}]}}))
 .on('reconcile', async request => {
   if(request.context.prompt!==undefined)throw new Error('Reconciliation received instructions');
   const result=await fs.readFile(path.join(request.context.cwd,'sdk-verifiable-'+request.context.run_id),'utf8');
   return {kind:'execution',value:{outcome:'completed',exit_code:0,result,confirmed:true}};
 })
 .on('execute', async (request, ctx) => {
   if(request.context.prompt==='needs-reconciliation') {
     await fs.appendFile(path.join(request.context.cwd,'sdk-submit-count-'+request.context.run_id),'submitted\\n');
     await fs.writeFile(path.join(request.context.cwd,'sdk-verifiable-'+request.context.run_id),'SDK verified result');
     return {kind:'execution',value:{outcome:'uncertain',reason:'Synthetic final reply requires verification'}};
   }

   if(request.context.prompt === 'wait-for-cancel') {
     await new Promise(resolve => {
       if(ctx.signal.aborted) return resolve();
       ctx.signal.addEventListener('abort',resolve,{once:true});
     });
     return {kind:'execution',value:{outcome:'cancelled',confirmed:true}};
   }
   const result = request.context.prompt;
   await ctx.result(result);
   return {kind:'execution',value:{outcome:'completed',exit_code:0,result:null,confirmed:true}};
 });
const asynchronous = new Agent(ASYNC_DESCRIPTOR)
 .on('detect', () => ({kind:'detection',value:{available:true,version:'synthetic',reason:null}}))
 .on('submit', async request => {
   const job=request.context.run_id;
   await fs.writeFile(path.join(request.context.cwd,'sdk-job-'+job),JSON.stringify({result:request.context.prompt,ready:Date.now()+100,cancelled:false}));
   return {kind:'execution',value:{outcome:'submitted',job_id:job}};
 })
 .on('poll', poll)
 .on('reconcile', poll)
 .on('cancel', async request => {
   const file=path.join(request.context.cwd,'sdk-job-'+request.context.job_id);
   const job=JSON.parse(await fs.readFile(file,'utf8'));job.cancelled=true;
   await fs.writeFile(file,JSON.stringify(job));
   return {kind:'execution',value:{outcome:'cancel_requested',job_id:request.context.job_id}};
 });
async function poll(request) {
 const job=JSON.parse(await fs.readFile(path.join(request.context.cwd,'sdk-job-'+request.context.job_id),'utf8'));
 if(job.cancelled)return {kind:'execution',value:{outcome:'cancelled',confirmed:true}};
 if(Date.now()<job.ready || job.result==='wait-for-native-job-cancel')return {kind:'execution',value:{outcome:'running',job_id:request.context.job_id}};
 return {kind:'execution',value:{outcome:'completed',exit_code:0,result:job.result,confirmed:true}};
}
await new Plugin().agent('sdk-agent',implementation).agent('sdk-async',asynchronous).serve();
""".replace('SDK_MODULE', json.dumps(sdk)).replace('ASYNC_DESCRIPTOR', json.dumps(async_descriptor)).replace('DESCRIPTOR', json.dumps(descriptor))
    (package / 'agent.ts').write_text(source)
    return package


def verify(context):
    api, task, apply = context['api'], context['task'], context['apply']
    trigger, final, check = context['trigger'], context['final'], context['check']
    identity = context['runner_id']
    agents = api('/agents')
    agent = next(a for a in agents if a['id'] == 'sdk-agent')
    assert agent['available'] and agent['trace_supported'] and agent['models_supported']
    # Hello advertises metadata before the Runner's query subscription is ready.
    models = wait(lambda: api(f'/runners/{identity}/agents/sdk-agent/models/query', 'POST', {'cwd': str(context['cwd'])}), 'SDK Agent model query channel')
    assert len(models['models']) == 1 and models['models'][0]['id'] == 'synthetic-model'
    apply(task('sdk-ordinary', 'sdk-agent', 'SDK-only final result'))
    completed = final(trigger('sdk-ordinary'))
    assert completed['result'] == 'SDK-only final result'
    assert completed['execution_snapshot']['parameters']['model'] == 'synthetic-model'
    check('SDK-only Agent registration, model query and durable ordinary Task result without Agent YAML')

    apply(task('sdk-cancel', 'sdk-agent', 'wait-for-cancel'))
    waiting = trigger('sdk-cancel')
    wait(lambda: api('/runs/' + waiting)['state'] == 'running', 'SDK execution started')
    apply(task('sdk-independent', 'sdk-agent', 'Other SDK Run survives cancellation'))
    independent = trigger('sdk-independent')
    api('/runs/' + waiting + '/cancel', 'POST', {})
    final(waiting, 'cancelled')
    assert final(independent)['result'] == 'Other SDK Run survives cancellation'
    check('Concurrent SDK Run cancellation is isolated to the selected execution')

    def node(name, prompt, dependencies=None):
        return {'id': name, 'name': name, 'runner': identity, 'agent': 'sdk-agent',
                'cwd': str(context['cwd']), 'prompt': prompt, 'pre': dependencies or [], 'timeout': '15s'}
    dag = {'version': 1, 'id': 'sdk-dag', 'name': 'SDK DAG', 'schedule': None, 'enabled': True,
           'execution': {'timeout': '30s', 'start_grace': '20s', 'overlap': 'skip', 'misfire': 'latest'},
           'dag': {'nodes': [node('first', 'SDK upstream'), node('second', '{{ nodes.first.result }} -> SDK downstream', ['first'])]}}
    apply(dag)
    completed = final(trigger('sdk-dag'))
    assert all(node['state'] == 'succeeded' for node in completed['nodes'])
    assert completed['result']['outputs'][0]['result'] == 'SDK upstream -> SDK downstream'
    check('DAG nodes use the SDK backend and consume the actual upstream result')

    apply(task('sdk-async-completed', 'sdk-async', 'SDK asynchronous report'))
    asynchronous = final(trigger('sdk-async-completed'))
    assert asynchronous['result'] == 'SDK asynchronous report'
    apply(task('sdk-async-cancelled', 'sdk-async', 'wait-for-native-job-cancel'))
    job = trigger('sdk-async-cancelled')
    wait(lambda: (context['cwd'] / ('sdk-job-' + job)).is_file(), 'SDK native job submission')
    api('/runs/' + job + '/cancel', 'POST', {})
    final(job, 'cancelled')
    saved = json.loads((context['cwd'] / ('sdk-job-' + job)).read_text())
    assert saved['cancelled']
    check('Asynchronous SDK job identity, polling, final result and confirmed native cancellation')

    # Exercise the real authenticated CLI -> Gateway -> v1 control -> SDK path.
    token_file = context['root'] / 'sdk-reconcile-admin.token'
    token_file.write_text(context['token'])
    token_file.chmod(0o600)
    def reconcile(run, node=None):
        args = [BIN, '--json', '--gateway', context['base'], '--workspace', context['workspace'],
                '--token-file', token_file, 'run', 'reconcile', run]
        if node is not None:
            args += ['--node', node]
        receipt = json.loads(command(args, context['env']))
        assert receipt['requested'] is True
    apply(task('sdk-verification', 'sdk-agent', 'needs-reconciliation'))
    uncertain = trigger('sdk-verification')
    final(uncertain, 'unknown')
    expect_status(lambda: context['ws'].call(context['base'], f'/runs/{uncertain}/reconcile', 'POST', {}, context['role_tokens']['viewer']), 403)
    reconcile(uncertain)
    assert final(uncertain)['result'] == 'SDK verified result'
    assert (context['cwd'] / ('sdk-submit-count-' + uncertain)).read_text().splitlines() == ['submitted']
    expect_status(lambda: api(f'/runs/{uncertain}/reconcile', 'POST', {}), 409)
    check('Operator-authorized CLI reconciliation reaches the saved SDK backend without resubmitting an unknown Run')

    graph = dict(dag, id='sdk-dag-verification', name='SDK DAG verification',
                 dag={'nodes': [node('first', 'needs-reconciliation'), node('second', '{{ nodes.first.result }} -> verified downstream', ['first'])]})
    apply(graph)
    graph_run = trigger(graph['id'])
    final(graph_run, 'unknown')
    reconcile(graph_run, 'first')
    verified = final(graph_run)
    assert verified['result']['outputs'][0]['result'] == 'SDK verified result -> verified downstream'
    check('DAG node reconciliation uses its exact execution and unblocks downstream SDK work')
