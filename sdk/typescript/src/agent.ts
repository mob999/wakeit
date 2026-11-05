/** Agent capability protocol. It never grants a plugin authority to set Run state. */
import type { Json, ObjectValue } from './index.ts'

export type AgentOperation = 'detect' | 'execute' | 'submit' | 'poll' | 'cancel' | 'reconcile' | 'models'
  | 'session_list' | 'session_inspect' | 'session_resume' | 'session_rename' | 'session_interactive'
export interface ParameterSpec { type: 'string'; label?: string; default?: string | null; enum?: string[] }
export interface AgentDescriptor {
  name: string; logo?: string | null; parameters?: Record<string, ParameterSpec>
  mode: 'sync' | 'async'; operations: AgentOperation[]
}
export interface AgentRequest {
  id: string; agent: string; operation: AgentOperation
  context: {
    cwd: string; deadline: string; run_id?: string; task_id?: string
    parameters: Record<string, string>; prompt?: string; session_id?: string
    session_title?: string; job_id?: string
  }
  input: Json
}
export interface SessionInfo { version: 1; id: string; cwd: string; persisted: boolean; title?: string | null }
export interface SessionEntry { id: string; cwd: string; title?: string | null; updated_at?: string | null }
export type ExecutionOutcome =
  | { outcome: 'not_started'; message: string }
  | { outcome: 'completed'; exit_code: number | null; result: Json; confirmed: boolean }
  | { outcome: 'submitted' | 'running'; job_id: string }
  | { outcome: 'cancel_requested'; job_id: string | null }
  | { outcome: 'cancelled'; confirmed: boolean }
  | { outcome: 'uncertain'; reason: string }
export type EnvironmentValue = { source: 'literal' | 'inherited' | 'credential'; value: string }
export type AgentReply =
  | { kind: 'detection'; value: { available: boolean; version: string | null; reason: string | null } }
  | { kind: 'execution'; value: ExecutionOutcome }
  | { kind: 'models'; value: { version: 1; models: { id: string; name: string; parameters: Record<string, string>; parameter_options: Record<string, string[]> }[] } }
  | { kind: 'sessions'; value: { version: 1; sessions: SessionEntry[]; next_cursor: string | null } }
  | { kind: 'session'; value: SessionInfo }
  | { kind: 'interactive'; value: { cwd: string; argv: string[]; env?: Record<string, EnvironmentValue> } }
  | { kind: 'renamed' }
export interface TraceNode {
  id: string; parent_id?: string | null; kind: 'message' | 'tool' | 'command'
  title: string; input: Json; output: Json; status: 'running' | 'succeeded' | 'failed' | 'cancelled'
  occurred_at: string; started_at?: string | null; finished_at?: string | null
}
export type RecordingIssue = 'content_shortened' | 'event_limit' | 'event_unsupported' | 'event_parse_failed' | 'event_too_large' | 'content_limit'
export interface LogRecord { stream?: string; text: string; diagnostic?: boolean; recording_issue?: RecordingIssue | null }
export type AgentOutput =
  | { version: 1; type: 'trace'; data: TraceNode }
  | { version: 1; type: 'log'; data: LogRecord }
  | { version: 1; type: 'session'; data: SessionInfo }
  | { version: 1; type: 'result'; data: Json }
export type NativeFact =
  | { event: 'started'; pid: number }
  | { event: 'input_submitted' }
  | { event: 'native_finished'; exit_code: number | null }
  | { event: 'start_failed'; message: string }
export interface AgentFact { request_id: string; run_id: string; seq: number; fact: NativeFact }
export type HostCall = <T = Json>(method: string, params?: ObjectValue, signal?: AbortSignal) => Promise<T>
export type AgentHandler = (request: AgentRequest, context: AgentContext) => AgentReply | Promise<AgentReply>
export const isExecution = (operation: AgentOperation) => ['execute', 'submit', 'session_resume'].includes(operation)

export class Agent {
  readonly descriptor: AgentDescriptor
  private readonly handlers = new Map<AgentOperation, AgentHandler>()
  constructor(descriptor: AgentDescriptor) { this.descriptor = descriptor }
  on(operation: AgentOperation, handler: AgentHandler) {
    if (this.handlers.has(operation)) throw new Error(`Duplicate Agent operation: ${operation}`)
    this.handlers.set(operation, handler); return this
  }
  validate() {
    const declared = [...this.descriptor.operations].sort()
    const registered = [...this.handlers.keys()].sort()
    if (new Set(declared).size !== declared.length || JSON.stringify(declared) !== JSON.stringify(registered)) throw new Error('Declared and registered Agent operations do not match')
    if (!declared.includes('detect') || (this.descriptor.mode === 'sync' ? !declared.includes('execute') : !declared.includes('submit') || !declared.includes('poll'))) throw new Error('Incomplete Agent execution capabilities')
    if (declared.some(op => op.startsWith('session_')) && !declared.includes('session_inspect')) throw new Error('Session operations require inspection')
  }
  async invoke(request: AgentRequest, context: AgentContext) {
    this.validate()
    const handler = this.handlers.get(request.operation)
    if (!handler) throw new Error('Agent operation is not supported')
    if (!request.context.cwd || Date.parse(request.context.deadline) <= Date.now()) throw new Error('Invalid or expired Agent context')
    if (isExecution(request.operation)) {
      if (!request.context.run_id || !request.context.task_id || request.context.prompt === undefined) throw new Error('Agent execution identity and input are required')
    } else if (request.context.prompt !== undefined) throw new Error('Non-execution operations cannot receive task instructions')
    return handler(request, context)
  }
}
export class AgentContext {
  readonly request: AgentRequest
  readonly signal: AbortSignal
  /** @internal Also stops outstanding host work when the request has completed. */
  readonly workSignal: AbortSignal
  private readonly call: HostCall
  private readonly requestId: string
  private readonly runId?: string
  private readonly execution: boolean
  private readonly outputController = new AbortController()
  private sequence = 1
  private finished = false
  private output: Promise<void> = Promise.resolve()
  constructor(request: AgentRequest, signal: AbortSignal, call: HostCall) {
    this.request = request; this.signal = signal; this.call = call
    this.requestId = request.id; this.runId = request.context.run_id
    this.execution = isExecution(request.operation)
    this.workSignal = AbortSignal.any([signal, this.outputController.signal])
  }
  /** @internal No new host work is permitted after cancellation or completion. */
  assertActive() {
    this.signal.throwIfAborted()
    if (this.finished) throw new Error('Agent request is already complete')
  }
  private enqueue(method: 'agent.record' | 'agent.fact', field: 'record' | 'fact', value: Json): Promise<void> {
    if (this.finished) return Promise.reject(new Error('Agent output is already complete'))
    if (!this.execution || !this.runId) return Promise.reject(new Error('Agent output requires an execution context'))
    // Bind identity at construction, and snapshot payloads before waiting for an ACK.
    const write = this.output.then(async () => {
      this.outputController.signal.throwIfAborted()
      await this.call(method, {
        request_id: this.requestId, run_id: this.runId!, seq: this.sequence, [field]: value,
      }, this.outputController.signal)
      this.sequence++
    })
    this.output = write
    // Fire-and-forget output is still drained by run()/flush(), including failures.
    void write.catch(() => {})
    return write
  }
  record(record: AgentOutput): Promise<void> {
    try {
      if (this.signal.aborted && !(record.type === 'session'
        || (record.type === 'log' && record.data.diagnostic === true)
        || (record.type === 'trace' && ['succeeded', 'failed', 'cancelled'].includes(record.data.status)))) {
        throw new Error('Only Agent cleanup records are permitted after cancellation')
      }
      const encoded = JSON.stringify(record)
      if (record.version !== 1 || Buffer.byteLength(encoded, 'utf8') > 1000 * 1024) throw new Error('Invalid or oversized Agent record')
      return this.enqueue('agent.record', 'record', JSON.parse(encoded) as Json)
    } catch (error) { return Promise.reject(error) }
  }
  fact(fact: NativeFact): Promise<void> {
    try {
      if (this.signal.aborted && !['native_finished', 'start_failed'].includes(fact.event)) {
        throw new Error('Only Agent cleanup facts are permitted after cancellation')
      }
      const encoded = JSON.stringify(fact)
      if (Buffer.byteLength(encoded, 'utf8') > 1000 * 1024) throw new Error('Oversized Agent fact')
      return this.enqueue('agent.fact', 'fact', JSON.parse(encoded) as Json)
    } catch (error) { return Promise.reject(error) }
  }
  /** Close admission before waiting for every earlier acknowledged record/fact. */
  async flush() { this.finished = true; await this.output }
  /** @internal Keep cleanup below the host's two-second cancellation window. */
  async run(handler: () => Promise<AgentReply>): Promise<AgentReply> {
    let timer: ReturnType<typeof setTimeout> | undefined
    let interrupt!: () => void
    let expire!: () => void
    const interrupted = new Promise<void>(resolve => { interrupt = resolve })
    const expired = new Promise<void>(resolve => { expire = resolve })
    const startCleanup = () => {
      if (timer !== undefined) return
      timer = setTimeout(() => {
        this.finished = true
        this.outputController.abort(this.signal.aborted ? this.signal.reason : new Error('Agent output acknowledgement timed out'))
        expire()
      }, 1500)
    }
    const onAbort = () => { interrupt(); startCleanup() }
    this.signal.addEventListener('abort', onAbort, { once: true })
    if (this.signal.aborted) onAbort()
    const settled = Promise.resolve().then(() => {
      this.assertActive()
      return handler()
    }).then(value => ({ value }), error => ({ error }))
    try {
      let outcome = await Promise.race([settled, interrupted])
      if (!outcome) outcome = await Promise.race([settled, expired])
      this.finished = true
      startCleanup()
      const drained = await Promise.race([
        this.output.then(() => ({}), error => ({ error })),
        expired.then(() => ({ error: this.outputController.signal.reason })),
      ])
      if (!('error' in drained) && this.signal.aborted && outcome && 'value' in outcome
        && outcome.value.kind === 'execution') {
        // A host stop is not evidence that native or remote work has stopped.
        // Preserve explicit cleanup evidence and job identity, but never turn a
        // late ordinary completion into success after the host requested a stop.
        switch (outcome.value.value.outcome) {
          case 'uncertain':
          case 'cancelled':
          case 'not_started':
          case 'submitted':
          case 'running':
          case 'cancel_requested':
            return outcome.value
          case 'completed':
            if (outcome.value.value.confirmed === false) return outcome.value
        }
      }
      this.signal.throwIfAborted()
      if ('error' in drained) throw drained.error
      if (!outcome) throw new Error('Agent cleanup timed out')
      if ('error' in outcome) throw outcome.error
      return outcome.value
    } finally {
      this.finished = true
      this.signal.removeEventListener('abort', onAbort)
      clearTimeout(timer)
      this.outputController.abort(new Error('Agent request is already complete'))
    }
  }
  trace(node: TraceNode) { return this.record({ version: 1, type: 'trace', data: node }) }
  log(log: LogRecord) { return this.record({ version: 1, type: 'log', data: log }) }
  session(session: SessionInfo) { return this.record({ version: 1, type: 'session', data: session }) }
  result(result: Json) { return this.record({ version: 1, type: 'result', data: result }) }
  config<T = ObjectValue>() {
    this.assertActive()
    return this.call<{ value: T; revision: number }>('config.get', { agent_request_id: this.requestId }, this.workSignal)
  }
  credential(slot: string) {
    this.assertActive()
    return this.call<Json>('credentials.get', { slot, agent_request_id: this.requestId }, this.workSignal)
  }
}
