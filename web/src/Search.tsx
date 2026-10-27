import type { TemplateRecord } from './templates/types'
import { ErrorToast } from './Toast'
import { t as tr } from './i18n'
import { useTranslation } from 'react-i18next'
import {
  createContext,
  useContext,
  useEffect,
  useState,
  useRef,
  useCallback,
  type ReactNode,
} from 'react'
import { useQuery } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Search, ListTodo, History, Monitor, Bot, Settings2, ArrowUpRight, ListTree } from './icons'
import {
  api,
  type Task,
  type Run,
  type Runner,
  type Agent,
  type User,
  stateNames,
  time,
} from './api'
import { TooltipTarget, Button, Field, Modal } from './ui'

export type Scope = 'presets' | 'tasks' | 'runs' | 'runners' | 'agents' | 'settings'
export type Filters = {
  q?: string
  state?: string
  runner?: string
  agent?: string
  enabled?: string
  online?: string
  available?: string
  mode?: string
}
export type PageResult<T> = { items: T[]; total: number; page: number; page_size: number }
const scopes = [
  {
    id: 'tasks',
    get name() {
      return tr('Tasks')
    },
    icon: ListTodo,
    path: '/tasks',
  },
  {
    id: 'presets',
    get name() {
      return tr('preset.title')
    },
    icon: ListTree,
    path: '/presets',
  },
  {
    id: 'runs',
    get name() {
      return tr('Runs')
    },
    icon: History,
    path: '/runs',
  },
  {
    id: 'runners',
    get name() {
      return tr('Runners')
    },
    icon: Monitor,
    path: '/runners',
  },
  {
    id: 'agents',
    get name() {
      return tr('Agents')
    },
    icon: Bot,
    path: '/runners',
  },
  {
    id: 'settings',
    get name() {
      return tr('Settings')
    },
    icon: Settings2,
    path: '/settings',
  },
] as const
export const settingSections = (role: User['role'], demo = false) => [
  {
    id: 'workspace',
    name: tr('工作区'),
    terms: 'workspace 时区 日志 保留 创建 退出 timezone retention create leave',
  },
  ...(!demo && role === 'admin'
    ? [
        {
          id: 'members',
          name: tr('成员'),
          terms: 'members admin operator viewer 管理员 读写 只读 邀请 角色',
        },
      ]
    : []),
  ...(!demo ? [{ id: 'tokens', name: tr('个人 API Key'), terms: 'key token api 凭据 cli' }] : []),
  ...(!demo && role !== 'viewer'
    ? [{ id: 'archived', name: tr('归档任务'), terms: 'archive archived task 归档 恢复' }]
    : []),
]
const SearchContext = createContext<{
  filters: Partial<Record<Scope, Filters>>
  setFilters: (v: Partial<Record<Scope, Filters>>) => void
  open: () => void
  registerGuard: (guard: () => Promise<boolean>) => () => void
  requestLeave: () => Promise<boolean>
}>({
  filters: {},
  setFilters: () => {},
  open: () => {},
  registerGuard: () => () => {},
  requestLeave: async () => true,
})
export const useSearchControls = () => useContext(SearchContext)
export const useFilters = (scope: Scope) => useContext(SearchContext).filters[scope] ?? {}
export function listPath(
  kind: 'tasks' | 'runs',
  filters: Filters,
  page: number,
  extra: Record<string, string> = {},
) {
  const params = new URLSearchParams({ page: String(page), page_size: '20', ...extra })
  for (const key of ['q', 'state', 'runner', 'agent', 'enabled'] as const)
    if (filters[key]) params.set(key, filters[key]!)
  return `/${kind}?${params}`
}
export function matches(text: string, q = '') {
  const haystack = text.toLocaleLowerCase()
  return q
    .trim()
    .toLocaleLowerCase()
    .split(/\s+/)
    .every((term) => haystack.includes(term))
}
export const runnerMatches = (r: Runner, f: Filters) =>
  matches(`${r.name} ${r.id} ${r.info.os ?? ''} ${r.agents.map((a) => a.name).join(' ')}`, f.q) &&
  (!f.online ||
    (f.online === 'revoked' ? r.revoked : !r.revoked && r.online === (f.online === 'online')))
export const agentMatches = (a: Agent, f: Filters) =>
  matches(`${a.name} ${a.id} ${a.runner ?? ''}`, f.q) &&
  (!f.runner || a.runner === f.runner) &&
  (!f.available || a.available === (f.available === 'true')) &&
  (!f.mode || a.mode === f.mode)
export function SearchProvider({ user, children }: { user: User; children: ReactNode }) {
  useTranslation()
  const [filters, setFilters] = useState<Partial<Record<Scope, Filters>>>({})
  const guardRef = useRef<(() => Promise<boolean>) | null>(null)
  const registerGuard = useCallback((guard: () => Promise<boolean>) => {
    guardRef.current = guard
    return () => {
      if (guardRef.current === guard) guardRef.current = null
    }
  }, [])
  const requestLeave = useCallback(() => guardRef.current?.() ?? Promise.resolve(true), [])
  const [open, setOpen] = useState(false)
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        setOpen((v) => !v)
      }
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [])
  return (
    <SearchContext.Provider
      value={{ filters, setFilters, open: () => setOpen(true), registerGuard, requestLeave }}
    >
      {children}
      {open && <SearchDialog user={user} onClose={() => setOpen(false)} />}
    </SearchContext.Provider>
  )
}
export function SearchLauncher({
  collapsed = false,
  onOpen,
}: {
  collapsed?: boolean
  onOpen?: () => void
}) {
  useTranslation()
  const { open, filters } = useContext(SearchContext)
  const active = Object.values(filters).some((f) => Object.values(f).some(Boolean))
  return (
    <TooltipTarget label={tr('搜索与筛选')} enabled={collapsed}>
      <button
        className="nav-item nav-search"
        onClick={() => {
          onOpen?.()
          open()
        }}
        aria-label={tr('搜索与筛选')}
      >
        <Search size={16} />
        <span>{tr('搜索')}</span>
        {active ? <span className="search-indicator" /> : <kbd>⌘K</kbd>}
      </button>
    </TooltipTarget>
  )
}
function SearchDialog({ user, onClose }: { user: User; onClose: () => void }) {
  useTranslation()
  const context = useContext(SearchContext)
  const navigate = useNavigate()
  const [q, setQ] = useState('')
  const [debounced, setDebounced] = useState('')
  const [selected, setSelected] = useState<Scope[]>([])
  const [draft, setDraft] = useState<Partial<Record<Scope, Filters>>>(context.filters)
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(q), 200)
    return () => clearTimeout(timer)
  }, [q])
  const includes = (scope: Scope) => !selected.length || selected.includes(scope)
  const f = (scope: Scope) => ({ ...(selected.includes(scope) ? draft[scope] : {}), q: debounced })
  const tasks = useQuery<PageResult<Task>>({
    queryKey: ['search', 'tasks', f('tasks')],
    queryFn: () => api(listPath('tasks', f('tasks'), 1)),
    enabled: includes('tasks'),
  })
  const presets = useQuery<PageResult<TemplateRecord>>({
    queryKey: ['search', 'presets', f('presets')],
    queryFn: () => api(`/task-templates?archived=false&q=${encodeURIComponent(debounced)}`),
    enabled: includes('presets'),
  })
  const runs = useQuery<PageResult<Run>>({
    queryKey: ['search', 'runs', f('runs')],
    queryFn: () => api(listPath('runs', f('runs'), 1)),
    enabled: includes('runs'),
  })
  const runners = useQuery<Runner[]>({ queryKey: ['runners'], queryFn: () => api('/runners') })
  const agents = useQuery<Agent[]>({ queryKey: ['agents'], queryFn: () => api('/agents') })
  const update = (scope: Scope, key: keyof Filters, value: string) =>
    setDraft((old) => ({ ...old, [scope]: { ...old[scope], [key]: value } }))
  const apply = (scope?: Scope) => {
    const targets = scope ? [scope] : selected.length ? selected : scopes.map((s) => s.id)
    context.setFilters({
      ...context.filters,
      ...Object.fromEntries(
        targets.map((s) => [s, { ...(selected.includes(s) ? draft[s] : {}), q }]),
      ),
    })
    onClose()
  }
  const commit = async (action: () => void) => {
    onClose()
    if (await context.requestLeave()) action()
  }
  const result = (scope: Scope, id: string) =>
    commit(() => {
      apply(scope)
      if (scope === 'presets') navigate({ to: '/presets', search: { template: id } })
      if (scope === 'tasks') navigate({ to: '/tasks', search: { task: id } })
      if (scope === 'runs') navigate({ to: '/runs', search: { run: id } })
      if (scope === 'runners') {
        context.setFilters({ ...context.filters, runners: { ...f('runners'), q }, agents: {} })
        navigate({ to: '/runners', search: { runner: id, section: 'overview' } })
      }
      if (scope === 'agents') {
        const [runner, agent] = JSON.parse(id) as [string, string]
        context.setFilters({ ...context.filters, agents: { ...f('agents'), q }, runners: {} })
        navigate({ to: '/runners', search: { runner, section: 'agents', agent } })
      }
      if (scope === 'settings') navigate({ to: '/settings', search: { section: id } })
    })
  const group = (
    scope: Scope,
    items: { id: string; name: string; meta: string }[],
    total = items.length,
    error?: Error | null,
    loading = false,
  ) => {
    const descriptor = scopes.find((s) => s.id === scope)!
    const Icon = descriptor.icon
    return (
      <section className="search-group content-card" key={scope}>
        <div className="search-group-heading table-head">
          <span>{descriptor.name}</span>
          <span>{total}</span>
        </div>
        {error ? (
          <ErrorToast>{error.message}</ErrorToast>
        ) : loading || q !== debounced ? (
          <p className="quiet">{tr('正在加载…')}</p>
        ) : !items.length ? (
          <p className="quiet search-empty">{tr('无匹配结果')}</p>
        ) : (
          items.map((item) => (
            <button className="search-result" key={item.id} onClick={() => result(scope, item.id)}>
              <Icon size={16} />
              <span>
                {item.name}
                <small>{item.meta}</small>
              </span>
              <ArrowUpRight size={13} />
            </button>
          ))
        )}
        {total > items.length && (
          <button
            className="search-more"
            onClick={() =>
              commit(() => {
                apply(scope)
                navigate({ to: descriptor.path, search: {} })
              })
            }
          >
            {tr('search.viewAll', { count: total })}
          </button>
        )}
      </section>
    )
  }
  return (
    <Modal open onOpenChange={(v) => !v && onClose()} title={tr('搜索')} wide>
      <div className="global-search-input">
        <Search size={18} />
        <input
          autoFocus
          aria-label={tr('搜索内容')}
          placeholder={tr('搜索…')}
          maxLength={160}
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
      </div>
      <div className="search-scopes" aria-label={tr('搜索范围')}>
        {scopes.map(({ id, name, icon: Icon }) => (
          <button
            key={id}
            aria-pressed={selected.includes(id)}
            onClick={() =>
              setSelected((old) => (old.includes(id) ? old.filter((s) => s !== id) : [...old, id]))
            }
          >
            <Icon size={14} />
            {name}
          </button>
        ))}
      </div>
      <div className="search-filter-groups">
        {selected
          .filter((s) => s !== 'settings' && s !== 'presets')
          .map((scope) => (
            <div className="search-filters" key={scope}>
              <span>{scopes.find((s) => s.id === scope)!.name}</span>
              {scope === 'tasks' && (
                <Field label={tr('任务状态')}>
                  <select
                    value={draft[scope]?.enabled ?? ''}
                    onChange={(e) => update(scope, 'enabled', e.target.value)}
                  >
                    <option value="">{tr('全部')}</option>
                    <option value="true">{tr('已启用')}</option>
                    <option value="false">{tr('actions.taskDisabled')}</option>
                  </select>
                </Field>
              )}
              {scope === 'runs' && (
                <Field label={tr('运行状态')}>
                  <select
                    value={draft[scope]?.state ?? ''}
                    onChange={(e) => update(scope, 'state', e.target.value)}
                  >
                    <option value="">{tr('全部')}</option>
                    {Object.entries(stateNames).map(([id, name]) => (
                      <option key={id} value={id}>
                        {name}
                      </option>
                    ))}
                  </select>
                </Field>
              )}
              {scope === 'runners' && (
                <Field label={tr('设备状态')}>
                  <select
                    value={draft[scope]?.online ?? ''}
                    onChange={(e) => update(scope, 'online', e.target.value)}
                  >
                    <option value="">{tr('全部')}</option>
                    <option value="online">{tr('在线')}</option>
                    <option value="offline">{tr('离线')}</option>
                    <option value="revoked">{tr('已吊销')}</option>
                  </select>
                </Field>
              )}
              {scope === 'agents' && (
                <>
                  <Field label={tr('适配状态')}>
                    <select
                      value={draft[scope]?.available ?? ''}
                      onChange={(e) => update(scope, 'available', e.target.value)}
                    >
                      <option value="">{tr('全部')}</option>
                      <option value="true">{tr('已检测到')}</option>
                      <option value="false">{tr('未检测到')}</option>
                    </select>
                  </Field>
                  <Field label={tr('模式')}>
                    <select
                      value={draft[scope]?.mode ?? ''}
                      onChange={(e) => update(scope, 'mode', e.target.value)}
                    >
                      <option value="">{tr('全部')}</option>
                      <option value="sync">{tr('同步')}</option>
                      <option value="async">{tr('异步')}</option>
                    </select>
                  </Field>
                </>
              )}
              {['tasks', 'runs', 'agents'].includes(scope) && (
                <Field label={tr('Runner')}>
                  <select
                    aria-label={`${scopes.find((s) => s.id === scope)!.name} Runner`}
                    value={draft[scope]?.runner ?? ''}
                    onChange={(e) => update(scope, 'runner', e.target.value)}
                  >
                    <option value="">{tr('全部 Runner')}</option>
                    {runners.data?.map((r) => (
                      <option key={r.id} value={r.name}>
                        {r.name}
                      </option>
                    ))}
                  </select>
                </Field>
              )}
              {['tasks', 'runs'].includes(scope) && (
                <Field label={tr('Agent')}>
                  <select
                    aria-label={`${scopes.find((s) => s.id === scope)!.name} Agent`}
                    value={draft[scope]?.agent ?? ''}
                    onChange={(e) => update(scope, 'agent', e.target.value)}
                  >
                    <option value="">{tr('全部 Agent')}</option>
                    {[...new Set(agents.data?.map((a) => a.id))].map((id) => (
                      <option key={id} value={id}>
                        {id}
                      </option>
                    ))}
                  </select>
                </Field>
              )}
            </div>
          ))}
      </div>
      <div className="search-results">
        {includes('tasks') &&
          group(
            'tasks',
            (tasks.data?.items ?? []).map((t) => ({
              id: t.id,
              name: t.config.name,
              meta: t.config.dag
                ? `DAG ${tr('dag.nodeCount', { count: t.config.dag.nodes.length })}`
                : `${t.config.runner} · ${t.config.agent}`,
            })),
            tasks.data?.total,
            tasks.error,
            tasks.isPending,
          )}
        {includes('presets') &&
          group(
            'presets',
            (presets.data?.items ?? []).map((row) => ({
              id: row.id,
              name: row.definition.name,
              meta: row.definition.description ?? '',
            })),
            presets.data?.total,
            presets.error,
            presets.isPending,
          )}
        {includes('runs') &&
          group(
            'runs',
            (runs.data?.items ?? []).map((r) => ({
              id: r.id,
              name: r.config.name,
              meta: `${stateNames[r.state] ?? r.state} · ${time(r.created_at)}`,
            })),
            runs.data?.total,
            runs.error,
            runs.isPending,
          )}
        {includes('runners') &&
          group(
            'runners',
            (runners.data ?? [])
              .filter((r) => runnerMatches(r, f('runners')))
              .map((r) => ({
                id: r.id,
                name: r.name,
                meta: r.revoked ? tr('已吊销') : r.online ? tr('在线') : tr('离线'),
              })),
            undefined,
            runners.error,
            runners.isPending,
          )}
        {includes('agents') &&
          group(
            'agents',
            (agents.data ?? [])
              .filter((a) => agentMatches(a, f('agents')))
              .map((a) => ({
                id: JSON.stringify([a.runner_id, a.id]),
                name: a.name,
                meta: a.runner ?? '',
              })),
            undefined,
            agents.error,
            agents.isPending,
          )}
        {includes('settings') &&
          group(
            'settings',
            settingSections(user.role, !!user.workspace?.is_demo)
              .filter((s) => matches(`${s.name} ${s.terms}`, debounced))
              .map((s) => ({ id: s.id, name: s.name, meta: '' })),
          )}
      </div>
      <div className="modal-footer search-footer">
        <Button
          onClick={async () => {
            if (!(await context.requestLeave())) return
            context.setFilters({})
            setDraft({})
            setQ('')
            setSelected([])
          }}
        >
          {tr('清除筛选')}
        </Button>
        <Button
          variant="solid"
          onClick={() =>
            commit(() => {
              apply()
              if (selected.length === 1) {
                if (selected[0] === 'agents')
                  navigate({ to: '/runners', search: { section: 'agents' } })
                else if (selected[0] === 'runners')
                  navigate({ to: '/runners', search: { section: 'overview' } })
                else navigate({ to: scopes.find((s) => s.id === selected[0])!.path })
              } else if (
                selected.length === 2 &&
                selected.includes('agents') &&
                selected.includes('runners')
              ) {
                navigate({ to: '/runners', search: { section: 'agents' } })
              }
            })
          }
        >
          {tr('应用')}
        </Button>
      </div>
    </Modal>
  )
}
