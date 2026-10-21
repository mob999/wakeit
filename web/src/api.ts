import { t as tr, locale } from './i18n'
export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message)
  }
}
// A document belongs to one workspace. Switching loads a new document, so delayed
// mutations and queries retain the scope in which they were initiated.
export const workspaceId = /^\/w\/([0-9a-f-]{36})(?:\/|$)/i.exec(location.pathname)?.[1]
export const workspaceBase = workspaceId ? `/w/${workspaceId}` : ''
export function apiUrl(path: string) {
  const global =
    /^\/(session(?:$|\?)|meta(?:$|\?)|changelog(?:$|\?)|workspaces(?:$|[/?])|instance\/|oauth\/|auth\/|device-authorizations\/|invites\/(accept|inspect)|register(?:$|\?)|schema\/|openapi\.json)/.test(
      path,
    )
  if (!global && !workspaceId) throw new Error(tr('请选择工作区'))
  return `/api/v1${global ? '' : `/workspaces/${workspaceId}`}${path}`
}
export async function api<T = any>(
  path: string,
  method = 'GET',
  body?: unknown,
  key?: string,
  signal?: AbortSignal,
): Promise<T> {
  const response = await fetch(apiUrl(path), {
    method,
    signal,
    credentials: 'same-origin',
    headers: {
      'Content-Type': 'application/json',
      'X-Wakeit-Request': '1',
      ...(method === 'POST' ? { 'Idempotency-Key': key ?? crypto.randomUUID() } : {}),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const value = await response.json()
  if (response.status === 401 && path !== '/session')
    window.dispatchEvent(new Event('wakeit:unauthorized'))
  if (!response.ok)
    throw new ApiError(
      response.status,
      value.error ? tr(value.error) : tr('请求失败 ({{v0}})', { v0: response.status }),
    )
  return value
}
export type Workspace = {
  id: string
  name: string
  role: 'admin' | 'operator' | 'viewer'
  is_demo: boolean
  is_default?: boolean
}
export type WorkspaceList = { items: Workspace[]; demo: Workspace | null }
export type User = {
  id: string
  username: string
  role: 'admin' | 'operator' | 'viewer'
  instance_admin?: boolean
  kind?: string
  workspace_id?: string | null
  workspace?: Workspace
}
export type ParameterSpec = {
  type: 'string'
  label: string
  default?: string | null
  enum: string[]
}
export type ModelEntry = {
  id: string
  name: string
  parameters: Record<string, string>
  parameter_options: Record<string, string[]>
}
export type ModelCatalog = { version: 1; models: ModelEntry[] }
export type DirectoryListing = {
  path: string | null
  parent: string | null
  breadcrumbs: { name: string; path: string }[]
  directories: { name: string; path: string }[]
  next_cursor: string | null
}
export type Agent = {
  id: string
  name: string
  logo?: string | null
  mode: 'sync' | 'async'
  digest: string
  available: boolean
  version?: string
  runner?: string
  runner_id?: string
  cancellable: boolean
  parameters: Record<string, ParameterSpec>
  trace_supported?: boolean
  models_supported: boolean
  session_supported?: boolean
  session_list_supported?: boolean
}
export type Runner = {
  id: string
  name: string
  online: boolean
  revoked: boolean
  last_seen: string | null
  agents: Agent[]
  directory_roots: string[]
  info: {
    os?: string
    arch?: string
    web_terminal?: number
    concurrency?: number
    active?: number
    unknown?: number
    version?: string
  }
}
export type Schedule = { cron?: string; every?: string; at?: string; timezone: string }
export type TaskConfig = {
  dag?: Dag
  version: number
  id: string
  name: string
  runner: string
  agent: string
  cwd: string
  prompt: string
  parameters: Record<string, string>
  session?: { id: string } | null
  enabled: boolean
  schedule: Schedule | null
  execution: {
    timeout: string | null
    start_grace: string
    overlap: 'skip' | 'allow'
    misfire: 'skip' | 'latest'
  }
}
export type DagNode = {
  id: string
  name: string
  pre: string[]
  runner: string
  agent: string
  cwd: string
  prompt: string
  parameters: Record<string, string>
  session?: { id: string } | null
  timeout: string
  max_retries?: number
}
export type Dag = { nodes: DagNode[]; results?: string[] | null }
export type DagIssue = { path: string; node_id?: string; message: string; cycle?: string[] }
export type DagRunNode = DagNode & {
  state: string
  execution_id?: string | null
  continuation_available?: boolean
  attempt_count?: number
  retry_at?: string | null
  reason?: string | null
  started_at?: string | null
  finished_at?: string | null
}
export type EditorLayout = { version: 1; positions: Record<string, { x: number; y: number }> }
export type Task = {
  editor_layout?: EditorLayout | null
  archived_at: string | null
  id: string
  revision: number
  config: TaskConfig
  next_at: string | null
  last_run?: { id: string; state: string; created_at: string }
}
export type Run = {
  parent_run_id?: string | null
  node_id?: string | null
  nodes?: DagRunNode[]
  task_archived: boolean
  id: string
  task_id: string
  state: string
  reason: string | null
  source: string
  created_at: string
  scheduled_at: string
  started_at: string | null
  finished_at: string | null
  result: unknown
  config: TaskConfig
  cancel_requested: boolean
  job_id: string | null
  session?: {
    id: string
    runner_id: string
    agent: string
    cwd: string
    ready: boolean
    reason?: string | null
    command?: string | null
    title?: string | null
  } | null
  execution_snapshot: {
    parameters: Record<string, string>
    cwd: string
    model: string
    trace_supported?: boolean
  } | null
}
export type SessionEntry = {
  id: string
  title?: string | null
  cwd: string
  updated_at?: string | null
}
export type SessionCatalog = { version: 1; sessions: SessionEntry[]; next_cursor?: string | null }
export type RunEvent = {
  run_id?: string
  node_id?: string | null
  execution_id?: string | null
  original_seq?: number
  id: number
  seq: number
  kind: string
  created_at: string
  data: {
    text?: string
    stream?: string
    message?: string
    state?: string
    result?: unknown
    status?: string
  }
}
export const terminal = (state: string) =>
  ['succeeded', 'warning', 'failed', 'cancelled', 'timed_out', 'skipped', 'interrupted'].includes(
    state,
  )
export const time = (value: string | null | undefined, timezone?: string) =>
  value
    ? new Date(value).toLocaleString(locale(), {
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
        hour12: false,
        ...(timezone ? { timeZone: timezone } : {}),
      })
    : '—'
export const stateNames: Record<string, string> = {
  get queued() {
    return tr('等待执行')
  },
  get starting() {
    return tr('启动中')
  },
  get running() {
    return tr('运行中')
  },
  get succeeded() {
    return tr('成功')
  },
  get warning() {
    return tr('dag.warning')
  },
  get waiting() {
    return tr('dag.waitingDependencies')
  },
  get failed() {
    return tr('失败')
  },
  get cancelled() {
    return tr('已取消')
  },
  get timed_out() {
    return tr('已超时')
  },
  get skipped() {
    return tr('已跳过')
  },
  get unknown() {
    return tr('状态待核对')
  },
  get interrupted() {
    return tr('已中断')
  },
}

export type Member = User & {
  active: boolean
  account_active?: boolean
  has_password?: boolean
  created_at: string
}
export type WorkspaceSettings = {
  timezone: string
  log_retention_days: number
  run_retention_days: number
}
export type ApiKey = { id: string; name: string; created_at: string; expires_at: string | null }

export type TraceNode = {
  id: string
  parent_id?: string | null
  kind: 'message' | 'tool' | 'command'
  status: 'running' | 'succeeded' | 'failed' | 'cancelled'
  title: string
  input: unknown
  output: unknown
  occurred_at: string
  started_at?: string | null
  finished_at?: string | null
}

export type MomentDay = { date: string; count: number }
export type MomentDays = {
  timezone: string
  today: string
  total_runs: number
  days: MomentDay[]
  next_before: string | null
}
export type MomentEntry = {
  continuation_available: boolean
  id: string
  task_id: string
  name: string
  state: string
  reason: string | null
  finished_at: string
  runner: string
  agent: string
  logo: string | null
  preview: string | null
  format: 'markdown' | 'json' | 'dag'
  node_count?: number
  outputs?: {
    node_id: string
    name: string
    state: string
    reason?: string | null
    preview: string | null
    format: 'markdown' | 'json'
    has_more: boolean
    truncated: boolean
  }[]
  has_more: boolean
  truncated: boolean
}
export type MomentEntries = {
  timezone: string
  date: string
  items: MomentEntry[]
  next_cursor: string | null
}
