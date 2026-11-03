import { Plugin, allow, deny, patch } from '../../../sdk/typescript/src/index.ts'
const plugin = new Plugin()
plugin.on('completed', async (input, ctx) => {
  const event = input as { id: string }
  const previous = await ctx.storage.get<number>('completed')
  await ctx.storage.set('completed', (previous?.value ?? 0) + 1, previous?.revision ?? 0)
  return { event: event.id, handled: true }
})
plugin.hook('prepare', (input) => input.prompt === 'reject' ? deny('synthetic_rejection', 'Synthetic input rejected') : patch({ prompt: `prepared:${input.prompt}` }))
plugin.hook('save', (input) => (input.config as { name?: string })?.name === 'reject' ? deny('synthetic_rejection', 'Synthetic definition rejected') : allow())
plugin.action('echo', async (input, ctx) => {
  const settings = await ctx.config.get()
  return { input, settings: settings.value }
})
plugin.action('interact', async (_, ctx) => {
  const answer = await ctx.ui.request({ kind: 'form', title: 'Synthetic form', schema: { type: 'object', properties: { answer: { type: 'string' } } } })
  return { answer }
})
plugin.action('wait', async (_, ctx) => {
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(resolve, 60000)
    ctx.signal.addEventListener('abort', () => { clearTimeout(timer); reject(new Error('cancelled')) }, { once: true })
  })
  return null
})
plugin.provider('fixture-models', () => ({ version: 1, models: [{ id: 'synthetic', name: 'Synthetic model', parameters: { model: 'synthetic' }, parameter_options: {} }] }))
plugin.action('host-queries', async (input, ctx) => {
  const tasks = await ctx.tasks.list()
  const stat = await ctx.files.call('stat', {path: input && typeof input==='object' && !Array.isArray(input) ? input.path : null})
  const chunk = await ctx.files.call('read', {path: input && typeof input==='object' && !Array.isArray(input) ? input.path : null, offset:0, limit:4})
  const models = await ctx.providers.call('query', {provider:'fixture-models',input:{}})
  return {tasks,stat,chunk,models}
})
plugin.action('oauth', async (input, ctx) => {
  const flow=await ctx.auth.call<{id:string;url:string;code?:string}>('begin',input as Record<string,any>)
  await ctx.ui.request(flow.code?{kind:'device_code',url:flow.url,code:flow.code,expires_at:new Date(Date.now()+60000).toISOString()}:{kind:'open_url',url:flow.url,label:'Synthetic authorization'})
  while(!ctx.signal.aborted){
    const status=await ctx.auth.call<{state:string}>('status',{id:flow.id})
    if(status.state==='authorized')break
    if(['failed','cancelled','expired'].includes(status.state))throw new Error(status.state)
    await new Promise(resolve=>setTimeout(resolve,100))
  }
  const params=input as Record<string,any>
  const token=await ctx.auth.call<Record<string,any>>('token',{slot:params.slot,token_url:params.token_url,client_id:params.client_id})
  return {authorized:typeof token.access_token==='string'}
})
plugin.action('postprocess-mode',async(input,ctx)=>{await ctx.storage.set('processing-mode',input);return null})
plugin.hook('publish',async(input,ctx)=>{
  const state=await ctx.storage.get<{reject?:boolean}>('processing-mode')
  return state?.value?.reject?deny('synthetic_postprocess','Synthetic result policy rejected the result'):allow()
})
await plugin.serve()
