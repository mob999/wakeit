import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { createInterface } from 'node:readline'
import { test } from 'node:test'

const sdk = new URL('../src/index.ts', import.meta.url).href
const complete = `{kind:'execution',value:{outcome:'completed',exit_code:0,result:'synthetic result',confirmed:true}}`

async function peer(t, handler, mode = 'sync') {
  const operation = mode === 'async' ? 'submit' : 'execute'
  const source = `
import { Plugin, Agent, Context } from ${JSON.stringify(sdk)};
let saved;
const implementation = new Agent({name:'Synthetic',mode:${JSON.stringify(mode)},operations:${JSON.stringify(mode === 'async' ? ['detect', 'submit', 'poll'] : ['detect', 'execute'])}})
 .on('detect',()=>({kind:'detection',value:{available:true,version:null,reason:null}}))
 .on(${JSON.stringify(operation)},(request,context)=>{saved=context;return (${handler})(request,context)});
if(${JSON.stringify(mode)}==='async')implementation.on('poll',()=>({kind:'execution',value:{outcome:'running',job_id:'synthetic-job'}}));
await new Plugin().agent('synthetic',implementation)
 .on('generic',async(input,context)=>context.storage.get('synthetic'))
 .lifecycleHandler('health',async(input)=>{
   if(!input.probe)return {};
   const rejected=[];
   for(const [name,call] of [
     ['record',()=>saved.log({text:'late synthetic output',diagnostic:true})],
     ['fact',()=>saved.fact({event:'native_finished',exit_code:null})],
     ['config',()=>saved.config()],
   ]){try{await call()}catch{rejected.push(name)}}
   return {rejected};
 }).serve();`
  const child = spawn(process.execPath, ['--input-type=module', '-e', source], { stdio: ['pipe', 'pipe', 'pipe'] })
  const messages = [], unread = [], waiters = []
  let stderr = ''
  child.stderr.on('data', data => { stderr += data })
  const closed = new Promise(resolve => child.once('close', (code, signal) => resolve({ code, signal })))
  const reader = createInterface({ input: child.stdout })
  reader.on('line', line => {
    const message = JSON.parse(line)
    messages.push(message)
    const index = waiters.findIndex(waiter => waiter.predicate(message))
    if (index < 0) unread.push(message)
    else { const [waiter] = waiters.splice(index, 1); clearTimeout(waiter.timer); waiter.resolve(message) }
  })
  t.after(async () => {
    child.stdin.end()
    const timer = setTimeout(() => child.kill('SIGKILL'), 200)
    const status = await closed
    clearTimeout(timer)
    for (const waiter of waiters) { clearTimeout(waiter.timer); waiter.reject(new Error('Synthetic peer closed')) }
    assert.equal(stderr, '', 'synthetic peer must not produce unhandled rejections or other diagnostics')
    assert.equal(status.code, 0, 'synthetic peer must exit cleanly')
  })
  const send = value => child.stdin.write(JSON.stringify({ jsonrpc: '2.0', ...value }) + '\n')
  const next = (predicate, timeout = 4000) => {
    const index = unread.findIndex(predicate)
    if (index >= 0) return Promise.resolve(unread.splice(index, 1)[0])
    return new Promise((resolve, reject) => {
      const waiter = { predicate, resolve, reject, timer: undefined }
      waiter.timer = setTimeout(() => { waiters.splice(waiters.indexOf(waiter), 1); reject(new Error(`Synthetic peer response timed out: ${stderr}`)) }, timeout)
      waiters.push(waiter)
    })
  }
  const request = (method, params) => {
    const id = randomUUID(); send({ id, method, params }); return { id, reply: () => next(message => message.id === id) }
  }
  const initialized = await request('initialize', { protocol_version: 1 }).reply()
  assert.equal(initialized.result.protocol_version, 1)
  return {
    messages, send, next, request,
    ack: message => send({ id: message.id, result: { seq: message.params.seq } }),
    gate: async () => { assert.deepEqual((await request('health', {}).reply()).result, {}) },
    invoke: (timeout = 10000, parameters = {}) => {
      const call = { id: randomUUID(), agent: 'synthetic', operation, context: {
        cwd: '/synthetic', deadline: new Date(Date.now() + timeout).toISOString(),
        run_id: randomUUID(), task_id: 'synthetic', parameters, prompt: 'synthetic instructions',
      }, input: null }
      send({ id: call.id, method: 'agent.invoke', params: call })
      return call
    },
  }
}

for (const failure of [false, true]) {
  test(`${failure ? 'failure' : 'success'} closes admission and drains mixed record/fact ACKs before replying`, async t => {
    const p = await peer(t, `async(request,context)=>{
      void context.log({text:'synthetic queued log'});
      void context.fact({event:'native_finished',exit_code:0});
      ${failure ? "throw new Error('synthetic failure')" : `return ${complete}`};
    }`)
    const call = p.invoke()
    const log = await p.next(message => message.method === 'agent.record')
    assert.equal(log.params.seq, 1)
    await p.gate()
    assert.equal(p.messages.some(message => message.method === 'agent.fact' || message.id === call.id), false)
    p.ack(log)
    const fact = await p.next(message => message.method === 'agent.fact')
    assert.deepEqual(fact.params, { request_id: call.id, run_id: call.context.run_id, seq: 2, fact: { event: 'native_finished', exit_code: 0 } })
    await p.gate()
    assert.equal(p.messages.some(message => message.id === call.id), false)
    p.ack(fact)
    const reply = await p.next(message => message.id === call.id)
    if (failure) assert.equal(reply.error.message, 'synthetic failure')
    else assert.equal(reply.result.value.result, 'synthetic result')
    const probe = await p.request('health', { probe: true }).reply()
    assert.deepEqual(probe.result.rejected, ['record', 'fact', 'config'])
    assert.equal(p.messages.filter(message => message.method).length, 2)
  })
}

test('cancel admits only bound cleanup output, rejects new work, and waits for its ACKs', async t => {
  const p = await peer(t, `async(request,context)=>{
    await context.fact({event:'started',pid:42});
    await new Promise(resolve=>context.signal.aborted?resolve():context.signal.addEventListener('abort',resolve,{once:true}));
    const denied=[];
    for(const [name,call] of [
      ['config',()=>context.config()], ['credential',()=>context.credential('synthetic')],
      ['external',()=>new Context().process.call('spawn',{command:'not-executed'})],
      ['started',()=>context.fact({event:'started',pid:99})],
      ['input',()=>context.fact({event:'input_submitted'})],
      ['result',()=>context.result('late result')],
      ['log',()=>context.log({text:'not cleanup'})],
      ['trace',()=>context.trace({id:'synthetic',status:'running'})],
    ]){try{await call()}catch{denied.push(name)}}
    request.id='changed';request.context.run_id='changed';request.operation='detect';
    void context.log({text:JSON.stringify(denied),diagnostic:true});
    await context.fact({event:'native_finished',exit_code:null});
    return ${complete};
  }`)
  const call = p.invoke()
  const started = await p.next(message => message.method === 'agent.fact')
  p.ack(started)
  p.send({ method: 'cancel', params: { id: call.id } })
  const log = await p.next(message => message.method === 'agent.record')
  assert.deepEqual(JSON.parse(log.params.record.data.text), ['config', 'credential', 'external', 'started', 'input', 'result', 'log', 'trace'])
  assert.equal(log.params.request_id, call.id)
  assert.equal(log.params.run_id, call.context.run_id)
  assert.equal(log.params.seq, 2)
  p.ack(log)
  const finished = await p.next(message => message.method === 'agent.fact')
  assert.equal(finished.params.seq, 3)
  assert.equal(finished.params.request_id, call.id)
  assert.equal(finished.params.run_id, call.context.run_id)
  await p.gate()
  assert.equal(p.messages.some(message => message.id === call.id), false)
  p.ack(finished)
  assert.equal((await p.next(message => message.id === call.id)).error.data.code, 'cancelled')
  assert.deepEqual((await p.request('health', { probe: true }).reply()).result.rejected, ['record', 'fact', 'config'])
  assert.deepEqual(p.messages.filter(message => message.method).map(message => message.method), ['agent.fact', 'agent.record', 'agent.fact'])
})

test('a deadline bounds an uncooperative handler and rejects late output', async t => {
  const p = await peer(t, `async(request,context)=>{await context.log({text:'synthetic entered'});await new Promise(()=>{})}`)
  const call = p.invoke(100)
  p.ack(await p.next(message => message.method === 'agent.record'))
  const reply = await p.next(message => message.id === call.id, 2500)
  assert.equal(reply.error.data.code, 'timeout')
  assert.deepEqual((await p.request('health', { probe: true }).reply()).result.rejected, ['record', 'fact', 'config'])
})

for (const cancel of [false, true]) {
  test(`${cancel ? 'cancelled' : 'completed'} requests cannot wait indefinitely for a missing ACK`, async t => {
    const p = await peer(t, `async(request,context)=>{void context.log({text:'synthetic no ACK'});return ${complete}}`)
    const call = p.invoke()
    const record = await p.next(message => message.method === 'agent.record')
    if (cancel) p.send({ method: 'cancel', params: { id: call.id } })
    const reply = await p.next(message => message.id === call.id, 2500)
    if (cancel) assert.equal(reply.error.data.code, 'cancelled')
    else assert.match(reply.error.message, /acknowledgement timed out/)
    p.ack(record) // A late ACK must neither reopen admission nor generate another reply.
    assert.deepEqual((await p.request('health', { probe: true }).reply()).result.rejected, ['record', 'fact', 'config'])
    assert.equal(p.messages.filter(message => message.id === call.id).length, 1)
    assert.equal(p.messages.filter(message => message.method).length, 1)
  })
}

test('generic extension host binding and cancellation remain compatible after an Agent request', async t => {
  const p = await peer(t, `async()=>(${complete})`)
  const call = p.invoke()
  assert.equal((await p.next(message => message.id === call.id)).result.kind, 'execution')
  for (const cancelled of [false, true]) {
    const invocation = { id: randomUUID(), instance_id: randomUUID(), operation_id: randomUUID(), kind: 'event', handler: 'generic', input: {}, deadline: new Date(Date.now() + 10000).toISOString(), expected_revision: null, actor: { kind: 'system', id: null, roles: [] }, causation_id: null }
    p.send({ id: invocation.id, method: 'invoke', params: invocation })
    const host = await p.next(message => message.method === 'storage.get')
    assert.deepEqual(host.params, { key: 'synthetic', invocation_id: invocation.id })
    if (cancelled) p.send({ method: 'cancel', params: { id: invocation.id } })
    else p.send({ id: host.id, result: { value: 'synthetic config', revision: 1 } })
    const reply = await p.next(message => message.id === invocation.id)
    if (cancelled) { assert.equal(reply.error.data.code, 'cancelled'); p.send({ id: host.id, result: null }) }
    else assert.deepEqual(reply.result, { value: 'synthetic config', revision: 1 })
  }
})


test('cancel interrupts an outstanding non-output host call but still acknowledges cleanup', async t => {
  const p = await peer(t, `async(request,context)=>{
    try { await context.config() }
    catch { await context.fact({event:'start_failed',message:'synthetic cancellation before native start'}) }
    return ${complete};
  }`)
  const call = p.invoke()
  const config = await p.next(message => message.method === 'config.get')
  assert.equal(config.params.agent_request_id, call.id)
  p.send({ method: 'cancel', params: { id: call.id } })
  const cleanup = await p.next(message => message.method === 'agent.fact')
  assert.deepEqual(cleanup.params, { request_id: call.id, run_id: call.context.run_id, seq: 1,
    fact: { event: 'start_failed', message: 'synthetic cancellation before native start' } })
  await p.gate()
  assert.equal(p.messages.some(message => message.id === call.id), false)
  p.ack(cleanup)
  assert.equal((await p.next(message => message.id === call.id)).error.data.code, 'cancelled')
  p.send({ id: config.id, result: { value: {}, revision: 1 } })
  await p.gate()
  assert.equal(p.messages.filter(message => message.id === call.id).length, 1)
})

test('cancelling one Agent request leaves another request and its sequence independent', async t => {
  const p = await peer(t, `async(request,context)=>{
    await context.fact({event:'started',pid:42});
    if(request.context.parameters.wait)await new Promise(resolve=>context.signal.aborted?resolve():context.signal.addEventListener('abort',resolve,{once:true}));
    await context.fact({event:'native_finished',exit_code:0});
    return ${complete};
  }`)
  const a = p.invoke(10000, { wait: 'yes' })
  const aStarted = await p.next(message => message.method === 'agent.fact' && message.params.request_id === a.id)
  p.ack(aStarted)
  const b = p.invoke()
  const bStarted = await p.next(message => message.method === 'agent.fact' && message.params.request_id === b.id)
  p.send({ method: 'cancel', params: { id: a.id } })
  const aFinished = await p.next(message => message.method === 'agent.fact' && message.params.request_id === a.id)
  p.ack(aFinished)
  assert.equal((await p.next(message => message.id === a.id)).error.data.code, 'cancelled')
  p.ack(bStarted)
  const bFinished = await p.next(message => message.method === 'agent.fact' && message.params.request_id === b.id)
  p.ack(bFinished)
  assert.equal((await p.next(message => message.id === b.id)).result.value.outcome, 'completed')
  for (const call of [a, b]) {
    const facts = p.messages.filter(message => message.method === 'agent.fact' && message.params.request_id === call.id)
    assert.deepEqual(facts.map(message => message.params.seq), [1, 2])
    assert.deepEqual(facts.map(message => message.params.run_id), [call.context.run_id, call.context.run_id])
  }
})


const cleanupOutcomes = [
  { name: 'uncertain', mode: 'sync', value: { outcome: 'uncertain', reason: 'synthetic remote cancellation not confirmed' } },
  { name: 'unconfirmed cancellation', mode: 'sync', value: { outcome: 'cancelled', confirmed: false } },
  { name: 'unconfirmed completion', mode: 'sync', value: { outcome: 'completed', exit_code: null, result: null, confirmed: false } },
  { name: 'not started', mode: 'sync', value: { outcome: 'not_started', message: 'synthetic request was not submitted' } },
  { name: 'confirmed cancellation', mode: 'sync', value: { outcome: 'cancelled', confirmed: true } },
  { name: 'submitted job identity', mode: 'async', value: { outcome: 'submitted', job_id: 'synthetic-submitted-job' } },
  { name: 'running job identity', mode: 'async', value: { outcome: 'running', job_id: 'synthetic-running-job' } },
  { name: 'cancel requested job identity', mode: 'async', value: { outcome: 'cancel_requested', job_id: 'synthetic-cancelling-job' } },
]
for (const stop of ['cancel', 'deadline']) {
  for (const { name, mode, value } of cleanupOutcomes) {
    test(`${stop} stop preserves ${name} after cleanup ACKs`, async t => {
      const expected = { kind: 'execution', value }
      const p = await peer(t, `async(request,context)=>{
        await context.log({text:'synthetic request entered'});
        await new Promise(resolve=>context.signal.aborted?resolve():context.signal.addEventListener('abort',resolve,{once:true}));
        void context.log({text:'synthetic cleanup complete',diagnostic:true});
        return ${JSON.stringify(expected)};
      }`, mode)
      const call = p.invoke(stop === 'deadline' ? 100 : 10000)
      const entered = await p.next(message => message.method === 'agent.record')
      assert.equal(entered.params.seq, 1)
      p.ack(entered)
      if (stop === 'cancel') p.send({ method: 'cancel', params: { id: call.id } })
      const cleanup = await p.next(message => message.method === 'agent.record')
      assert.equal(cleanup.params.seq, 2)
      assert.equal(cleanup.params.request_id, call.id)
      assert.equal(cleanup.params.run_id, call.context.run_id)
      await p.gate()
      assert.equal(p.messages.some(message => message.id === call.id), false, 'native evidence must not bypass the output ACK barrier')
      p.ack(cleanup)
      assert.deepEqual((await p.next(message => message.id === call.id)).result, expected)
      assert.deepEqual((await p.request('health', { probe: true }).reply()).result.rejected, ['record', 'fact', 'config'])
      assert.equal(p.messages.filter(message => message.method).length, 2)
    })
  }
  test(`${stop} stop rejects ordinary late success after cleanup ACKs`, async t => {
    const p = await peer(t, `async(request,context)=>{
      await context.log({text:'synthetic request entered'});
      await new Promise(resolve=>context.signal.aborted?resolve():context.signal.addEventListener('abort',resolve,{once:true}));
      void context.log({text:'synthetic cleanup complete',diagnostic:true});
      return ${complete};
    }`)
    const call = p.invoke(stop === 'deadline' ? 100 : 10000)
    p.ack(await p.next(message => message.method === 'agent.record'))
    if (stop === 'cancel') p.send({ method: 'cancel', params: { id: call.id } })
    const cleanup = await p.next(message => message.method === 'agent.record')
    await p.gate()
    assert.equal(p.messages.some(message => message.id === call.id), false)
    p.ack(cleanup)
    const reply = await p.next(message => message.id === call.id)
    assert.equal(reply.error.data.code, stop === 'cancel' ? 'cancelled' : 'timeout')
    assert.equal(reply.result, undefined)
  })
}

test('stop preserves an uncertain result when cancellation arrives during its ACK barrier', async t => {
  const expected = { kind: 'execution', value: { outcome: 'uncertain', reason: 'synthetic native status unknown' } }
  const p = await peer(t, `async()=>{saved.log({text:'synthetic observation'});return ${JSON.stringify(expected)}}`)
  const call = p.invoke()
  const observation = await p.next(message => message.method === 'agent.record')
  p.send({ method: 'cancel', params: { id: call.id } })
  await p.gate()
  assert.equal(p.messages.some(message => message.id === call.id), false)
  p.ack(observation)
  assert.deepEqual((await p.next(message => message.id === call.id)).result, expected)
})
