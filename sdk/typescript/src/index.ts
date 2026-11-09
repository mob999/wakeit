import { Agent, AgentContext, type AgentRequest } from './agent.ts'
export { Agent, AgentContext } from './agent.ts'
export type { AgentDescriptor, AgentOperation, AgentRequest, AgentReply, ExecutionOutcome, AgentOutput, NativeFact, AgentFact, TraceNode, SessionInfo } from './agent.ts'
/** Wakeit extension SDK. This is a stdio peer, not an in-process Runner module. */
import { createInterface } from 'node:readline'
import { AsyncLocalStorage } from 'node:async_hooks'
import { randomUUID } from 'node:crypto'

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json }
export type ObjectValue = { [key: string]: Json }
export type Decision = { decision: 'allow' } | { decision: 'deny'; code: string; message: string; paths?: string[] } | { decision: 'patch'; value: ObjectValue }
export type Receipt<T = Json> = { id: string; state: string; revision: number; result?: T; error?: { code: string; message: string; retryable: boolean; paths: string[] } }
export type Event<T = Json> = {
  id: string; event_type: string; schema_version: number; workspace_id: string
  aggregate_type: string; aggregate_id: string; aggregate_revision: number
  occurred_at: string; committed_at: string; operation_id: string | null
  causation_id: string | null; correlation_id: string | null; source_runner_id: string | null
  origin: { kind: string; id: string | null; roles: string[] }; data: T
  snapshot_ref?: { event_id: string }; idempotency_key?: string
}
export interface RuntimeInfo {
  runner_id: string; instance_id: string; sdk_protocol: number; version: string
  platform: string; arch: string; invocation_id: string | null
  /** Gateway-advertised browser URL, not the Runner connection address. Older hosts may omit it. */
  public_url?: string | null
}
export type Invocation = {
  id: string; instance_id: string; operation_id: string; kind: 'event' | 'hook' | 'action' | 'provider'
  handler: string; input: Json; deadline: string; expected_revision: number | null
  actor: { kind: string; id: string | null; roles: string[] }; causation_id: string | null
}
export type Interaction =
  | { kind: 'progress'; message: string; fraction?: number }
  | { kind: 'form'; title: string; schema: Json; value?: Json }
  | { kind: 'secret'; slot: string; label: string }
  | { kind: 'confirm'; title: string; description: string }
  | { kind: 'choice'; title: string; options: {value:string;label:string;description?:string}[] }
  | { kind: 'open_url'; url: string; label: string }
  | { kind: 'device_code'; url: string; code: string; expires_at: string }
  | { kind: 'download'; reference: string; name: string }
  | { kind: 'navigate'; path: string }
  | { kind: 'toast'; message: string }
export class PluginError extends Error {
  code: string; retryable: boolean; paths: string[]
  constructor(code: string, message: string, options: { retryable?: boolean; paths?: string[] } = {}) {
    super(message); this.code = code; this.retryable = options.retryable ?? false; this.paths = options.paths ?? []
  }
}
const invocationContext = new AsyncLocalStorage<{ invocation?: Invocation; agent?: AgentContext; agentRequestId?: string; signal: AbortSignal }>()
const pending = new Map<string, { resolve: (value: Json) => void; reject: (error: Error) => void }>()
const controllers = new Map<string, AbortController>()
let initialized = false
function write(value: unknown) { process.stdout.write(JSON.stringify({ jsonrpc: '2.0', ...value as object }) + '\n') }
async function host<T = Json>(method: string, params: ObjectValue = {}): Promise<T> {
  const context = invocationContext.getStore()
  context?.agent?.assertActive()
  return rpc<T>(method, {
    ...params,
    ...(context?.invocation ? { invocation_id: context.invocation.id } : {}),
    ...(context?.agentRequestId ? { agent_request_id: context.agentRequestId } : {}),
  }, context?.agent?.workSignal ?? context?.signal)
}
async function rpc<T = Json>(method: string, params: ObjectValue = {}, signal?: AbortSignal): Promise<T> {
  signal?.throwIfAborted()
  const id = randomUUID()
  return new Promise<T>((resolve, reject) => {
    const onAbort = () => { pending.delete(id); reject(signal?.reason ?? new Error('Invocation cancelled')) }
    const done = () => signal?.removeEventListener('abort', onAbort)
    pending.set(id, { resolve: (value) => { done(); resolve(value as T) }, reject: (error) => { done(); reject(error) } })
    signal?.addEventListener('abort', onAbort, { once: true })
    try { write({ id, method, params }) }
    catch (error) { pending.delete(id); done(); reject(error) }
  })
}
export class Domain {
  private readonly namespace: string
  constructor(namespace: string) { this.namespace = namespace }
  call<T = Json>(operation: string, params: ObjectValue = {}) { return host<T>(`${this.namespace}.${operation}`, params) }
  list<T = Json>(query: ObjectValue = {}) { return this.call<T>('list', { query }) }
  get<T = Json>(id: string) { return this.call<T>('get', { id }) }
  create<T = Receipt>(id: string, body: Json, requestKey = randomUUID()) { return this.call<T>('create', { id, body, request_key: requestKey }) }
  update<T = Receipt>(id: string, body: Json, requestKey = randomUUID()) { return this.call<T>('update', { id, body, request_key: requestKey }) }
  invoke<T = Receipt>(operation: string, id: string, body: Json = {}, requestKey = randomUUID()) { return this.call<T>(operation, { id, body, request_key: requestKey }) }
  async *watch<T = Json>(id: string, options: { signal?: AbortSignal; interval?: number } = {}): AsyncGenerator<T> {
    const signal=options.signal ?? invocationContext.getStore()?.signal
    let previous: string | undefined
    while(!signal?.aborted){
      const value=await this.get<T>(id);const serialized=JSON.stringify(value)
      if(serialized!==previous){previous=serialized;yield value}
      if(value && typeof value==='object' && 'state' in value && ['succeeded','failed','warning','cancelled','skipped','timed_out','interrupted'].includes(String(value.state)))return
      await new Promise<void>((resolve,reject)=>{const timer=setTimeout(()=>{signal?.removeEventListener('abort',abort);resolve()},options.interval??1000);const abort=()=>{clearTimeout(timer);reject(signal?.reason??new Error('cancelled'))};signal?.addEventListener('abort',abort,{once:true})})
    }
  }

}
export class Runs extends Domain {
  constructor() { super('runs') }
  reconcile(id: string, requestKey = randomUUID()) {
    return this.call<{ ok: true; run_id: string; requested: true }>('reconcile', { id, request_key: requestKey })
  }
}
export class Runtime extends Domain {
  constructor() { super('runtime') }
  info() { return this.call<RuntimeInfo>('info') }
}
export class Context {
  readonly invocation?: Invocation
  readonly signal: AbortSignal
  constructor(invocation?: Invocation, signal: AbortSignal = new AbortController().signal) { this.invocation = invocation; this.signal = signal }
  tasks = new Domain('tasks'); templates = new Domain('templates'); runs = new Runs()
  dag = new Domain('dag'); schedule = new Domain('schedule'); workspace = new Domain('workspace')
  members = new Domain('members'); devices = new Domain('devices'); agents = new Domain('agents')
  models = new Domain('models'); sessions = new Domain('sessions'); results = new Domain('results')
  files = new Domain('files'); artifacts = new Domain('artifacts'); plugins = new Domain('plugins')
  events = new Domain('events'); auth = new Domain('auth'); runtime = new Runtime()
  http = new Domain('http'); process = new Domain('process'); providers = new Domain('providers')
  config = {
    get: <T = ObjectValue>() => host<{ value: T; revision: number }>('config.get'),
    save: (update: ObjectValue) => host<Receipt>('config.save', {update}),
  }
  storage = {
    get: <T = Json>(key: string) => host<{ value: T; revision: number } | null>('storage.get', { key }),
    list: (prefix = '', cursor?: string) => host('storage.list', { prefix, ...(cursor ? { cursor } : {}) }),
    set: (key: string, value: Json, expectedRevision?: number) => host<{ revision: number }>('storage.set', { key, value, ...(expectedRevision === undefined ? {} : { expected_revision: expectedRevision }) }),
    delete: (key: string, expectedRevision?: number) => host('storage.delete', { key, ...(expectedRevision === undefined ? {} : { expected_revision: expectedRevision }) }),
  }
  credentials = {
    status: () => host<Record<string, boolean>>('credentials.status'),
    get: (slot: string) => host<Json>('credentials.get', { slot }),
    set: (slot: string, value: string) => host('credentials.set', { slot, value }),
    remove: (slot: string) => host('credentials.remove', { slot }),
  }
  ui = { request: <T = Json>(request: Interaction) => host<T>('ui.request', { request: request as unknown as Json }) }
  streams = {
    events: async function* (runId: string, options: { after?:number; kind?:string; signal?:AbortSignal; interval?:number } = {}): AsyncGenerator<Json> {
      const signal=options.signal??invocationContext.getStore()?.signal
      let cursor=options.after??0
      while(!signal?.aborted){
        const rows=await host<(ObjectValue&{id:number})[]>('runs.events',{id:runId,query:{after:cursor,...(options.kind?{kind:options.kind}:{})}})
        for(const row of rows){if(typeof row.id==='number')cursor=Math.max(cursor,row.id);yield row}
        if(rows.length<500){
          const run=await host<{state:string}>('runs.get',{id:runId})
          if(['succeeded','failed','warning','cancelled','skipped','timed_out','interrupted'].includes(run.state))return
          await new Promise<void>((resolve,reject)=>{const timer=setTimeout(()=>{signal?.removeEventListener('abort',abort);resolve()},options.interval??500);const abort=()=>{clearTimeout(timer);reject(signal?.reason??new Error('cancelled'))};signal?.addEventListener('abort',abort,{once:true})})
        }
      }
    }
  }
  log = (level: 'debug' | 'info' | 'warn' | 'error', message: string, data: ObjectValue = {}) => host('log.write', { level, message, data })
  health = (state: string, message?: string) => host('health.report', { state, ...(message ? { message } : {}) })
}
type Handler = (input: Json, ctx: Context) => Json | void | Promise<Json | void>
type HookHandler = (input: ObjectValue, ctx: Context) => Decision | Promise<Decision>
export type Lifecycle = 'activate' | 'deactivate' | 'dispose' | 'validateConfig' | 'applyConfig' | 'migrateConfig' | 'health'
export class Plugin {
  private agents = new Map<string, Agent>()
  agent(id: string, implementation: Agent) { implementation.validate(); this.add(this.agents, id, implementation); return this }
  private handlers = new Map<string, Handler>()
  private hooks = new Map<string, HookHandler>()
  private actions = new Map<string, Handler>()
  private providers = new Map<string, Handler>()
  private lifecycle = new Map<Lifecycle, Handler>()
  on(id: string, handler: Handler) { this.add(this.handlers, id, handler); return this }
  hook(id: string, handler: HookHandler) { this.add(this.hooks, id, handler); return this }
  action(id: string, handler: Handler) { this.add(this.actions, id, handler); return this }
  provider(id: string, handler: Handler) { this.add(this.providers, id, handler); return this }
  lifecycleHandler(name: Lifecycle, handler: Handler) { this.add(this.lifecycle, name, handler); return this }
  private add<T>(map: Map<string, T>, id: string, value: T) { if (map.has(id)) throw new Error(`Duplicate registration: ${id}`); map.set(id, value) }
  async serve() {
    const reader = createInterface({ input: process.stdin, crlfDelay: Infinity })
    reader.on('line', (line) => {
      void (async () => {
        let message: { id?: string; method?: string; params?: ObjectValue; result?: Json; error?: { code: string|number; message: string; data?:{code?:string;retryable?:boolean;paths?:string[]} } }
        try { message = JSON.parse(line) } catch { write({ id: null, error: { code: -32700, message: 'Invalid JSON', data:{code:'invalid_json'} } }); return }
        const id = message.id
        if (!message.method) {
          const waiting = id ? pending.get(id) : undefined
          if (waiting && id) { pending.delete(id); message.error ? waiting.reject(new PluginError(message.error.data?.code??String(message.error.code), message.error.message, {retryable:message.error.data?.retryable,paths:message.error.data?.paths})) : waiting.resolve(message.result ?? null) }
          return
        }
        try {
          const params = message.params ?? {}
          let result: unknown = null
          if (message.method === 'initialize') {
            if (params.protocol_version !== 1 || initialized) throw new PluginError('incompatible_protocol', 'Unsupported protocol or duplicate initialization')
            for (const implementation of this.agents.values()) implementation.validate()
            initialized = true
            result = { protocol_version: 1, agents: Object.fromEntries([...this.agents].map(([id, implementation]) => [id, implementation.descriptor])), handlers: [...this.handlers.keys()], hooks: [...this.hooks.keys()], actions: [...this.actions.keys()], providers: [...this.providers.keys()] }
          } else if (message.method === 'cancel') {
            const controller = controllers.get(String(params.id)); controller?.abort(new PluginError('cancelled', 'Invocation cancelled'))
          } else if (message.method === 'agent.invoke') {
            if (!initialized) throw new PluginError('not_initialized', 'Initialize before invoking')
            const call = params as unknown as AgentRequest
            const implementation = this.agents.get(call.agent)
            if (!implementation) throw new PluginError('unknown_agent', 'Agent is not registered')
            const controller = new AbortController()
            const remaining = Date.parse(call.context.deadline) - Date.now()
            if (!Number.isFinite(remaining) || remaining <= 0) throw new PluginError('expired', 'Agent request expired')
            const timer = setTimeout(() => controller.abort(new PluginError('timeout', 'Agent request deadline exceeded')), remaining)
            controllers.set(call.id, controller)
            const context = new AgentContext(call, controller.signal, rpc)
            try {
              result = await invocationContext.run({ agent: context, agentRequestId: call.id, signal: controller.signal },
                () => context.run(() => implementation.invoke(call, context)))
            }
            finally { clearTimeout(timer); controllers.delete(call.id) }
          } else if (message.method === 'invoke') {
            if (!initialized) throw new PluginError('not_initialized', 'Initialize before invoking')
            const call = params as unknown as Invocation
            const table = call.kind === 'event' ? this.handlers : call.kind === 'hook' ? this.hooks : call.kind === 'action' ? this.actions : this.providers
            const handler = table.get(call.handler)
            if (!handler) throw new PluginError('unknown_handler', `Unknown ${call.kind}: ${call.handler}`)
            const controller = new AbortController(); controllers.set(call.id, controller)
            const remaining = Date.parse(call.deadline) - Date.now()
            if (remaining <= 0) throw new PluginError('expired', 'Invocation expired')
            const timer = setTimeout(() => controller.abort(new PluginError('timeout', 'Invocation deadline exceeded')), remaining)
            try { result = await invocationContext.run({ invocation: call, signal: controller.signal }, () => handler(call.input as ObjectValue, new Context(call, controller.signal))) }
            finally { clearTimeout(timer); controllers.delete(call.id) }
          } else if (this.lifecycle.has(message.method as Lifecycle)) {
            result = await this.lifecycle.get(message.method as Lifecycle)!(params, new Context())
          } else if (!['activate', 'deactivate', 'dispose', 'validateConfig', 'applyConfig', 'migrateConfig', 'health'].includes(message.method)) {
            throw new PluginError('method_not_found', `Unknown method: ${message.method}`)
          }
          if (id) write({ id, result: result ?? null })
        } catch (error) {
          const value = error instanceof PluginError ? error : new PluginError('plugin_error', error instanceof Error ? error.message : String(error))
          if (id) write({ id, error: { code: -32000, message: value.message, data: { code:value.code,retryable: value.retryable, paths: value.paths } } })
        }
      })()
    })
    await new Promise<void>((resolve) => reader.on('close', resolve))
    for (const c of controllers.values()) c.abort(new PluginError('shutdown', 'Host disconnected'))
    for (const call of pending.values()) call.reject(new PluginError('shutdown', 'Host disconnected'))
  }
}
export const allow = (): Decision => ({ decision: 'allow' })
export const deny = (code: string, message: string, paths: string[] = []): Decision => ({ decision: 'deny', code, message, paths })
export const patch = (value: ObjectValue): Decision => ({ decision: 'patch', value })

export const retryAfter = (seconds: number, message: string): Json => ({ outcome: 'retry_after', seconds, message })
export const permanentFailure = (message: string): Json => ({ outcome: 'permanent_failure', message })
export const uncertain = (message: string): Json => ({ outcome: 'uncertain', message })

export interface TaskRecord { id: string; revision: number; enabled: boolean; config: ObjectValue }
export interface RunRecord { id: string; state: string; config: ObjectValue; result: Json; reason: string | null }
export interface ModelCatalog { version: 1; models: { id: string; name: string; parameters: Record<string,string>; parameter_options: Record<string,string[]> }[] }
export interface Page<T> { items:T[]; total?:number; page?:number; page_size?:number }
