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
await plugin.serve()
