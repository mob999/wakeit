import { t as tr } from './i18n'
import { useTranslation } from 'react-i18next'
import { MomentsPage, MomentsVisit } from './moments/MomentsPage'
import { ContentCard } from './ContentCard'
import { Button, IconButton, Field, Modal, TooltipProvider } from './ui'
import { CopyValue } from './runs/CopyValue'
import { PageActions } from './PageHeader'
import { LanguageSwitch } from './LanguageSwitch'
import { Layout } from './Layout'
import { WorkspaceHome } from './Workspaces'
import { InvitationPage } from './InvitationPage'
import { InstanceAccounts } from './settings/InstanceAccounts'
import { DeviceAuthorization } from './onboarding/DeviceAuthorization'
import { TaskEditor } from './TaskEditor'
import { DagEditor } from './dag/DagEditor'
import { DagTaskOverview } from './dag/DagTaskOverview'
import * as Dropdown from '@radix-ui/react-dropdown-menu'
import { SettingsPage } from './settings/SettingsPage'
import { Split } from './Workbench'
import { RunnersPage } from './runners/RunnersPage'
import { Status } from './Status'
import { RunDetail } from './runs/RunDetail'
import { RunListRow, RunIdentity, RunDevice, RunAgent } from './runs/RunListRow'
import { SearchProvider, useFilters, listPath, type PageResult } from './Search'
import { Pagination } from './Pagination'
import React, { useState, useEffect, createContext, useContext, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider, useQuery, useMutation } from '@tanstack/react-query'
import {
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
  Outlet,
  Link,
  useNavigate,
  useRouterState,
} from '@tanstack/react-router'
import {
  AlarmClock,
  Plus,
  Play,
  Pause,
  ChevronRight,
  Terminal,
  X,
  RefreshCw,
  ArrowUpRight,
  FileCog,
  CalendarCheck,
  Clock,
  AlertCircle,
  Archive,
} from 'lucide-react'
import YAML from 'yaml'
import {
  api,
  ApiError,
  workspaceId,
  workspaceBase,
  type Workspace,
  type User,
  type Task,
  type Agent,
  type Run,
  time,
} from './api'
import './style.css'

const client = new QueryClient({
  defaultOptions: {
    queries: {
      queryKeyHashFn: (key) => JSON.stringify([workspaceId ?? 'account', key]),
      retry: (n, e) => !(e instanceof ApiError && [401, 403].includes(e.status)) && n < 2,
      refetchOnWindowFocus: true,
    },
  },
})
const UserContext = createContext<User | null>(null)
const ToastContext = createContext<(message: string) => void>(() => {})
const useUser = () => useContext(UserContext)!
const useToast = () => useContext(ToastContext)
function useList<T>(key: string, path: string) {
  return useQuery<T[]>({ queryKey: [key], queryFn: () => api(path), refetchInterval: 3000 })
}
function Empty({
  title,
  detail,
  children,
}: {
  title: string
  detail?: string
  children?: ReactNode
}) {
  useTranslation()
  return (
    <div className="empty">
      <AlarmClock size={27} />
      <h3>{title}</h3>
      {detail && <p>{detail}</p>}
      {children}
    </div>
  )
}
function CopyButton({ value }: { value: string }) {
  useTranslation()
  return <CopyValue value={value} toast={useToast()} />
}

function App() {
  useTranslation()
  const me = useQuery<User>({
    queryKey: ['me'],
    queryFn: async () => {
      const account = await api<User>('/session')
      if (!workspaceId) return account
      const workspace = await api<Workspace>(`/workspaces/${workspaceId}`)
      return { ...account, workspace, role: workspace.role }
    },
    retry: false,
    refetchInterval: 10000,
  })
  useEffect(() => {
    const expired = () => {
      client.clear()
      location.assign(location.pathname === '/authorize/device' ? location.href : '/')
    }
    window.addEventListener('wakeit:unauthorized', expired)
    return () => window.removeEventListener('wakeit:unauthorized', expired)
  }, [])
  useEffect(() => {
    if (me.data && me.error instanceof ApiError && me.error.status === 401) {
      client.clear()
      location.assign('/')
    }
  }, [me.error, me.data])
  const [toast, setToast] = useState('')
  const [dark, setDark] = useState(
    () =>
      localStorage.getItem('wakeit-theme') === 'dark' ||
      (!localStorage.getItem('wakeit-theme') && matchMedia('(prefers-color-scheme: dark)').matches),
  )
  useEffect(() => {
    document.documentElement.dataset.theme = dark ? 'dark' : 'light'
    localStorage.setItem('wakeit-theme', dark ? 'dark' : 'light')
  }, [dark])
  useEffect(() => {
    if (toast) {
      const timer = setTimeout(() => setToast(''), 5000)
      return () => clearTimeout(timer)
    }
  }, [toast])
  const path = useRouterState({ select: (s) => s.location.pathname })
  return (
    <ToastContext.Provider value={setToast}>
      {path === '/invite' ? (
        <InvitationPage user={me.data} />
      ) : me.isPending ? (
        <Empty title={tr('连接 Wakeit…')} />
      ) : me.error ? (
        me.error instanceof ApiError && me.error.status === 401 ? (
          <Login />
        ) : me.error instanceof ApiError && [403, 404].includes(me.error.status) && workspaceId ? (
          <WorkspaceHome
            user={me.data ?? { id: '', username: '', role: 'viewer' }}
            error={tr('无法访问此工作区')}
          />
        ) : (
          <Empty title={tr('无法连接 Gateway')} detail={me.error.message}>
            <Button onClick={() => me.refetch()}>
              <RefreshCw size={14} />
              {tr('重试')}
            </Button>
          </Empty>
        )
      ) : (
        <UserContext.Provider value={me.data!}>
          <SearchProvider user={me.data!}>
            {path === '/authorize/device' ? (
              <DeviceAuthorization user={me.data!} />
            ) : !workspaceId ? (
              path === '/instance' && me.data?.instance_admin ? (
                <InstanceAccounts user={me.data!} />
              ) : (
                <WorkspaceHome user={me.data!} />
              )
            ) : (
              <Layout
                user={me.data!}
                dark={dark}
                onTheme={() => setDark((v) => !v)}
                onLogout={() =>
                  api('/session', 'DELETE')
                    .then(() => {
                      client.clear()
                      location.assign('/')
                    })
                    .catch((e) => setToast(e.message))
                }
              >
                <MomentsVisit>
                  <Outlet />
                </MomentsVisit>
              </Layout>
            )}
          </SearchProvider>
        </UserContext.Provider>
      )}
      {toast && (
        <div className="toast" role="status">
          <AlertCircle size={15} />
          {toast}
          <IconButton label={tr('关闭提示')} onClick={() => setToast('')}>
            <X />
          </IconButton>
        </div>
      )}
    </ToastContext.Provider>
  )
}
function Login() {
  useTranslation()
  const meta = useQuery<{ auth?: { mode: string; allow_registration: boolean } }>({
    queryKey: ['meta'],
    queryFn: () => api('/meta'),
  })
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [signup, setSignup] = useState(false)
  const returnTo = new URLSearchParams(location.search).get('return_to')
  const next = returnTo?.startsWith('/') && !returnTo.startsWith('//') ? returnTo : undefined
  const login = useMutation({
    mutationFn: async () => {
      if (signup) {
        if (password !== confirm) throw new Error(tr('两次输入的密码不一致'))
        await api('/register', 'POST', { username: username.trim(), password })
      }
      await api('/session', 'POST', { username: username.trim(), password })
    },
    onSuccess: () => {
      setPassword('')
      setConfirm('')
      if (next) location.assign(next)
      else client.invalidateQueries({ queryKey: ['me'] })
    },
  })
  return (
    <div className="login-wrap">
      <LanguageSwitch className="login-language" />
      <div className="login-brand">
        <span>Wakeit</span>
      </div>
      <form
        className="login-card content-card"
        onSubmit={(event) => {
          event.preventDefault()
          if (!login.isPending) login.mutate()
        }}
      >
        <h1 className="card-header">{signup ? tr('创建账号') : tr('欢迎回来')}</h1>
        <div className="card-body">
          {new URLSearchParams(location.search).get('auth_error') === 'github_denied' && (
            <p className="error" role="alert">
              {tr('GitHub 授权已取消')}
            </p>
          )}
          {meta.isPending ? (
            <span className="quiet">{tr('加载中…')}</span>
          ) : meta.error ? (
            <>
              <p className="error">{meta.error.message}</p>
              <Button type="button" onClick={() => void meta.refetch()}>
                {tr('重试')}
              </Button>
            </>
          ) : meta.data?.auth?.mode === 'github' ? (
            <Button
              variant="solid"
              type="button"
              onClick={() =>
                location.assign(
                  `/api/v1/auth/github/start?return_to=${encodeURIComponent(next ?? location.pathname + location.search)}`,
                )
              }
            >
              {tr('使用 GitHub 登录')}
            </Button>
          ) : (
            <>
              <Field label={tr('用户名')}>
                <input
                  aria-label={tr('用户名')}
                  autoComplete="username"
                  value={username}
                  onChange={(event) => setUsername(event.target.value)}
                  required
                  disabled={login.isPending}
                />
              </Field>
              <Field label={tr('密码')} hint={signup ? tr('至少 12 个字符') : undefined}>
                <input
                  aria-label={tr('密码')}
                  type="password"
                  autoComplete={signup ? 'new-password' : 'current-password'}
                  value={password}
                  onChange={(event) => setPassword(event.target.value)}
                  required
                  minLength={signup ? 12 : undefined}
                  disabled={login.isPending}
                />
              </Field>
              {signup && (
                <Field label={tr('确认密码')}>
                  <input
                    aria-label={tr('确认密码')}
                    type="password"
                    autoComplete="new-password"
                    value={confirm}
                    onChange={(event) => setConfirm(event.target.value)}
                    required
                    minLength={12}
                    disabled={login.isPending}
                  />
                </Field>
              )}
              {login.error && (
                <div className="error" role="alert">
                  {login.error.message}
                </div>
              )}
              <Button variant="solid" type="submit" disabled={login.isPending}>
                {login.isPending ? tr('正在连接…') : signup ? tr('创建账号') : tr('登录工作区')}
                <ArrowUpRight size={14} />
              </Button>
              {meta.data?.auth?.allow_registration && (
                <Button
                  type="button"
                  disabled={login.isPending}
                  onClick={() => {
                    setSignup(!signup)
                    setPassword('')
                    setConfirm('')
                    login.reset()
                  }}
                >
                  {signup ? tr('登录已有账号') : tr('创建账号')}
                </Button>
              )}
            </>
          )}
          {new URLSearchParams(location.search).has('login_error') && (
            <p className="error" role="alert">
              {tr('GitHub 登录失败，请重试')}
            </p>
          )}
        </div>
      </form>
    </div>
  )
}

function TasksPage() {
  useTranslation()
  const agents = useList<Agent>('agents', '/agents')
  const filters = useFilters('tasks')
  const routeSearch = tasksRoute.useSearch()
  const [page, setPage] = useState(1)
  const tasks = useQuery<PageResult<Task>>({
    queryKey: ['tasks', filters, page],
    queryFn: () => api(listPath('tasks', filters, page)),
    refetchInterval: 3000,
  })
  useEffect(() => {
    setPage(1)
    setSelected(null)
  }, [JSON.stringify(filters)])
  useEffect(() => {
    if (tasks.data && page > 1 && (page - 1) * 20 >= tasks.data.total)
      setPage(Math.max(1, Math.ceil(tasks.data.total / 20)))
  }, [tasks.data, page])

  const settings = useQuery<{ timezone: string }>({
    queryKey: ['settings'],
    queryFn: () => api('/settings'),
  })
  const [selected, setSelected] = useState<string | null>(null)
  const [editor, setEditor] = useState<Task | 'new' | null>(null)
  const [archiving, setArchiving] = useState<Task | null>(null)
  const [tab, setTab] = useState('overview')
  const u = useUser()
  const editable = u.role !== 'viewer'
  const toast = useToast()
  const navigate = useNavigate()
  const requested = routeSearch.task ?? selected
  const direct = useQuery<Task>({
    queryKey: ['tasks', 'detail', requested],
    queryFn: () => api(`/tasks/${encodeURIComponent(requested!)}`),
    enabled: !!requested,
    refetchInterval: 3000,
  })
  const task =
    tasks.data?.items.find((t) => t.id === requested) ??
    (requested ? direct.data : undefined) ??
    tasks.data?.items[0]
  const [historyPage, setHistoryPage] = useState(1)
  useEffect(() => setHistoryPage(1), [task?.id])
  const history = useQuery<PageResult<Run>>({
    queryKey: ['task-runs', task?.id, tab === 'runs' ? historyPage : 1],
    queryFn: () => api(listPath('runs', {}, tab === 'runs' ? historyPage : 1, { task: task!.id })),
    enabled: !!task,
    refetchInterval: 3000,
  })
  const action = useMutation({
    mutationFn: async ({ task, kind }: { task: Task; kind: 'trigger' | 'toggle' }) =>
      kind === 'trigger'
        ? api<{ id: string }>(`/tasks/${task.id}/runs`, 'POST', {})
        : api(`/tasks/${task.id}`, 'PUT', {
            config: { ...task.config, enabled: !task.config.enabled },
            expected_revision: task.revision,
          }),
    onSuccess: (v, vars) => {
      client.invalidateQueries({ queryKey: ['tasks'] })
      if (vars.kind === 'trigger') navigate({ to: '/runs', search: { run: v.id } })
    },
    onError: (e) => toast(e.message),
  })
  const archive = useMutation({
    mutationFn: (target: Task) =>
      api(`/tasks/${target.id}/archive`, 'POST', { expected_revision: target.revision }),
    onSuccess: () => {
      setArchiving(null)
      setSelected(null)
      client.invalidateQueries({ queryKey: ['tasks'] })
      client.invalidateQueries({ queryKey: ['archived-tasks'] })
      client.invalidateQueries({ queryKey: ['run'] })
      navigate({ to: '/tasks', search: {} })
    },
  })
  return (
    <>
      <Modal
        open={!!archiving}
        title={tr('task.archive')}
        description={tr(
          '归档「{{v0}}」后停止调度，取消排队执行；已开始的执行继续，可在设置中恢复。',
          { v0: archiving?.config.name ?? '' },
        )}
        onOpenChange={(v) => {
          if (!v && !archive.isPending) setArchiving(null)
        }}
      >
        <div className="modal-body">
          {archive.error && (
            <p className="error" role="alert">
              {archive.error.message}
            </p>
          )}
        </div>
        <div className="modal-footer">
          <Button disabled={archive.isPending} onClick={() => setArchiving(null)}>
            {tr('取消')}
          </Button>
          <Button
            variant="solid"
            disabled={archive.isPending}
            onClick={() => archiving && archive.mutate(archiving)}
          >
            {archive.isPending ? tr('正在归档…') : tr('归档')}
          </Button>
        </div>
      </Modal>
      <Split
        label={tr('Tasks')}
        crumbs={task ? [{ label: task.config.name }] : []}
        detailOpen={!!routeSearch.task}
        onBack={() => {
          setSelected(null)
          navigate({ to: '/tasks', search: {} })
        }}
        list={
          <>
            <div className="pane-heading">
              <h1>{tr('Tasks')}</h1>
              {editable && (
                <Dropdown.Root>
                  <Dropdown.Trigger asChild>
                    <IconButton label={tr('新建任务')} disabled={!settings.data}>
                      <Plus />
                    </IconButton>
                  </Dropdown.Trigger>
                  <Dropdown.Portal>
                    <Dropdown.Content className="dropdown" align="end">
                      <Dropdown.Item onSelect={() => setEditor('new')}>{tr('任务')}</Dropdown.Item>
                      <Dropdown.Item onSelect={() => navigate({ to: '/tasks/dag', search: {} })}>
                        {tr('dag.task')}
                      </Dropdown.Item>
                    </Dropdown.Content>
                  </Dropdown.Portal>
                </Dropdown.Root>
              )}
            </div>
            <div className="list-scroll">
              {tasks.error ? (
                <div className="error">{tasks.error.message}</div>
              ) : tasks.isPending ? (
                <p className="quiet padded">{tr('正在加载…')}</p>
              ) : tasks.data?.total === 0 ? (
                <Empty title={tr('暂无任务')} />
              ) : (
                tasks.data?.items.map((t) => (
                  <button
                    key={t.id}
                    className={`task-row ${task?.id === t.id ? 'active' : ''}`}
                    onClick={() => {
                      setSelected(t.id)
                      navigate({ to: '/tasks', search: { task: t.id } })
                      setTab('overview')
                    }}
                  >
                    <div className="row-title">
                      <span>{t.config.name}</span>
                      <Status state={t.last_run?.state ?? 'not_run'} />
                    </div>
                    <div className="row-meta">
                      <RunIdentity run={t} agents={agents.data} />
                    </div>
                    <div className="row-meta next">
                      <Clock size={12} />
                      {t.config.enabled ? time(t.next_at) : tr('已暂停')}
                      <ChevronRight size={13} />
                    </div>
                  </button>
                ))
              )}
            </div>
            <Pagination
              page={page}
              total={tasks.data?.total ?? 0}
              pending={tasks.isFetching}
              onChange={(p) => {
                setPage(p)
                setSelected(null)
                navigate({ to: '/tasks', search: {} })
              }}
            />
          </>
        }
      >
        {!task ? (
          <Empty title={tr('暂无任务')}>
            {editable && (
              <Button onClick={() => setEditor('new')}>
                <Plus size={14} />
                {tr('新建任务')}
              </Button>
            )}
          </Empty>
        ) : (
          <>
            <PageActions>
              <CopyValue label={tr('复制 Task ID')} value={task.id} toast={toast} />
              {task.archived_at && <span className="quiet">{tr('已归档')}</span>}
              {editable && !task.archived_at && (
                <>
                  <IconButton
                    label={tr('编辑')}
                    onClick={() =>
                      task.config.dag
                        ? navigate({ to: '/tasks/dag', search: { task: task.id } })
                        : setEditor(task)
                    }
                  >
                    <FileCog />
                  </IconButton>
                  <IconButton
                    label={task.config.enabled ? tr('暂停任务') : tr('task.resume')}
                    disabled={action.isPending}
                    onClick={() => action.mutate({ task, kind: 'toggle' })}
                  >
                    {task.config.enabled ? <Pause /> : <CalendarCheck />}
                  </IconButton>
                  <IconButton
                    label={tr('立即执行')}
                    disabled={action.isPending}
                    onClick={() => action.mutate({ task, kind: 'trigger' })}
                  >
                    <Play />
                  </IconButton>
                  <IconButton
                    label={tr('task.archive')}
                    disabled={action.isPending}
                    onClick={() => {
                      archive.reset()
                      setArchiving(task)
                    }}
                  >
                    <Archive />
                  </IconButton>
                </>
              )}
            </PageActions>
            <div className="tabs">
              {[
                ['overview', tr('概览')],
                ['runs', tr('执行历史')],
                ['config', tr('配置')],
              ].map(([key, label]) => (
                <button
                  key={key}
                  className={tab === key ? 'active' : ''}
                  onClick={() => setTab(key)}
                >
                  {label}
                </button>
              ))}
            </div>
            <div className="detail-scroll">
              {tab === 'overview' ? (
                <>
                  <ContentCard title={tr('调度')}>
                    <div className="summary-grid">
                      <div>
                        <span className="quiet">{tr('下一次执行')}</span>
                        <strong>{task.config.enabled ? time(task.next_at) : tr('已暂停')}</strong>
                        <small>{task.config.schedule?.timezone ?? tr('手动执行')}</small>
                      </div>
                      <div>
                        <span className="quiet">{tr('上次结果')}</span>
                        <strong>
                          <Status state={task.last_run?.state ?? 'not_run'} />
                        </strong>
                        <small>{time(task.last_run?.created_at)}</small>
                      </div>
                    </div>
                  </ContentCard>
                  {task.config.dag ? (
                    <DagTaskOverview config={task.config} agents={agents.data} />
                  ) : (
                    <>
                      <ContentCard title={tr('指令')}>
                        <pre className="task-prompt">{task.config.prompt}</pre>
                      </ContentCard>
                      <ContentCard title={tr('执行内容')}>
                        <dl className="properties">
                          <dt>{tr('Runner')}</dt>
                          <dd>{task.config.runner}</dd>
                          <dt>{tr('Agent')}</dt>
                          <dd className="mono">{task.config.agent}</dd>
                          <dt>{tr('工作目录')}</dt>
                          <dd className="mono wrap">{task.config.cwd}</dd>
                          <dt>{tr('参数')}</dt>
                          <dd className="mono wrap">
                            {Object.entries(task.config.parameters ?? {})
                              .map(([key, value]) => `${key}: ${value}`)
                              .join(' · ') || tr('默认配置')}
                          </dd>
                          <dt>{tr('会话')}</dt>
                          <dd className={task.config.session ? 'mono wrap' : 'quiet'}>
                            {task.config.session?.id ?? tr('每次新建')}
                          </dd>
                          <dt>{tr('执行时间')}</dt>
                          <dd className="mono">
                            {task.config.schedule
                              ? (task.config.schedule.cron ??
                                task.config.schedule.every ??
                                time(task.config.schedule.at))
                              : tr('手动执行')}
                          </dd>
                          <dt>{tr('超时')}</dt>
                          <dd>{task.config.execution.timeout}</dd>
                        </dl>
                      </ContentCard>
                    </>
                  )}
                  <RunTable title={tr('最近执行')} runs={history.data?.items.slice(0, 5) ?? []} />
                </>
              ) : tab === 'runs' ? (
                <>
                  <RunTable title={tr('执行历史')} runs={history.data?.items ?? []} />
                  <Pagination
                    page={historyPage}
                    total={history.data?.total ?? 0}
                    pending={history.isFetching}
                    onChange={setHistoryPage}
                  />
                </>
              ) : (
                <ContentCard
                  title={tr('Task YAML')}
                  actions={<CopyButton value={YAML.stringify(task.config)} />}
                >
                  <pre className="code">{YAML.stringify(task.config)}</pre>
                </ContentCard>
              )}
            </div>
          </>
        )}
      </Split>
      {editor && (
        <TaskEditor
          task={editor === 'new' ? undefined : editor}
          defaultTimezone={settings.data?.timezone ?? 'Asia/Shanghai'}
          onClose={() => setEditor(null)}
        />
      )}
    </>
  )
}
function RunTable({ runs, title }: { runs: Run[]; title: string }) {
  useTranslation()
  const agents = useList<Agent>('agents', '/agents')
  return (
    <section className="run-table content-card">
      <div className="run-table-head run-table-row table-head">
        <h3>{title}</h3>
        <span>{tr('执行设备')}</span>
        <span>{tr('Agent')}</span>
        <span>{tr('执行时间')}</span>
        <span />
      </div>
      {runs.length ? (
        <>
          {runs.map((r) => (
            <Link to="/runs" search={{ run: r.id }} key={r.id} className="run-table-row">
              <Status state={r.state} />
              {r.config.dag ? (
                <span className="dag-table-identity">
                  <RunIdentity run={r} />
                </span>
              ) : (
                <>
                  <RunDevice run={r} agents={agents.data} />
                  <RunAgent run={r} agents={agents.data} />
                </>
              )}
              <span className="run-table-time">{time(r.started_at ?? r.created_at)}</span>
              <ChevronRight size={13} />
            </Link>
          ))}
        </>
      ) : (
        <p className="quiet card-state">{tr('暂无执行记录')}</p>
      )}
    </section>
  )
}
function RunsPage() {
  useTranslation()
  const user = useUser()
  const toast = useToast()
  const agents = useList<Agent>('agents', '/agents')
  const search = runsRoute.useSearch()
  const navigate = useNavigate()
  const filters = useFilters('runs')
  const [page, setPage] = useState(1)
  useEffect(() => setPage(1), [JSON.stringify(filters)])
  const runs = useQuery<PageResult<Run>>({
    queryKey: ['runs', filters, page],
    queryFn: () => api(listPath('runs', filters, page)),
    refetchInterval: 2000,
  })
  useEffect(() => {
    if (runs.data && page > 1 && (page - 1) * 20 >= runs.data.total)
      setPage(Math.max(1, Math.ceil(runs.data.total / 20)))
  }, [runs.data, page])
  const selected = search.run ?? runs.data?.items[0]?.id
  const detail = useQuery<Run>({
    queryKey: ['run', selected],
    queryFn: () => api(`/runs/${selected}`),
    enabled: !!selected,
    refetchInterval: 2000,
  })
  const selectedRun = runs.data?.items.find((r) => r.id === selected) ?? detail.data
  return (
    <Split
      label={tr('Runs')}
      crumbs={
        selectedRun
          ? [
              {
                label: selectedRun.config.name,
                onClick: () => navigate({ to: '/tasks', search: { task: selectedRun.task_id } }),
              },
              { label: time(selectedRun.started_at ?? selectedRun.created_at) },
            ]
          : []
      }
      detailOpen={!!search.run}
      onBack={() => navigate({ to: '/runs', search: {} })}
      list={
        <>
          <div className="pane-heading">
            <h1>{tr('Runs')}</h1>
          </div>
          <div className="list-scroll">
            {runs.error ? (
              <p className="error padded">{runs.error.message}</p>
            ) : !runs.data?.items.length ? (
              <Empty title={tr('暂无执行记录')} />
            ) : (
              runs.data?.items.map((r) => (
                <RunListRow
                  key={r.id}
                  run={r}
                  agents={agents.data}
                  selected={selected === r.id}
                  onSelect={() => navigate({ to: '/runs', search: { run: r.id } })}
                />
              ))
            )}
          </div>
          <Pagination
            page={page}
            total={runs.data?.total ?? 0}
            pending={runs.isFetching}
            onChange={(p) => {
              setPage(p)
              navigate({ to: '/runs', search: {} })
            }}
          />
        </>
      }
    >
      {selected ? (
        <RunDetail key={selected} id={selected} user={user} toast={toast} />
      ) : (
        <Empty title={tr('暂无执行记录')} />
      )}
    </Split>
  )
}
function SettingsScreen() {
  useTranslation()
  return <SettingsPage user={useUser()} toast={useToast()} />
}

const rootRoute = createRootRoute({ component: App })
const momentsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  component: () => <MomentsPage toast={useToast()} />,
})
const tasksRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/tasks',
  validateSearch: (s: Record<string, unknown>): { task?: string } => ({
    task: typeof s.task === 'string' ? s.task : undefined,
  }),
  component: TasksPage,
})
const dagRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/tasks/dag',
  validateSearch: (s: Record<string, unknown>): { task?: string } => ({
    task: typeof s.task === 'string' ? s.task : undefined,
  }),
  component: DagScreen,
})
function DagScreen() {
  const user = useUser(),
    navigate = useNavigate(),
    search = dagRoute.useSearch()
  const settings = useQuery<{ timezone: string }>({
    queryKey: ['settings'],
    queryFn: () => api('/settings'),
  })
  const task = useQuery<Task>({
    queryKey: ['tasks', 'detail', search.task],
    queryFn: () => api(`/tasks/${encodeURIComponent(search.task!)}`),
    enabled: !!search.task,
  })
  if (user.role === 'viewer' || user.workspace?.is_demo)
    return <Empty title={tr('无法访问此工作区')} />
  if (settings.error || task.error)
    return <Empty title={tr('加载失败')} detail={(settings.error || task.error)?.message} />
  if (!settings.data || (search.task && !task.data)) return <Empty title={tr('正在加载…')} />
  if (task.data && (!task.data.config.dag || task.data.archived_at))
    return <Empty title={tr('dag.fixErrors')} />
  return (
    <DagEditor
      key={search.task ?? 'new'}
      task={task.data}
      timezone={settings.data.timezone}
      onClose={() => navigate({ to: '/tasks', search: { task: search.task } })}
      onSaved={(id) => navigate({ to: '/tasks', search: { task: id } })}
    />
  )
}
const runsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/runs',
  validateSearch: (search: Record<string, unknown>): { run?: string } => ({
    run: typeof search.run === 'string' ? search.run : undefined,
  }),
  component: RunsPage,
})
const runnersRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/runners',
  validateSearch: (
    s: Record<string, unknown>,
  ): { runner?: string; section?: 'overview' | 'agents'; agent?: string } => ({
    runner: typeof s.runner === 'string' ? s.runner : undefined,
    section: s.section === 'agents' || s.section === 'overview' ? s.section : undefined,
    agent: typeof s.agent === 'string' ? s.agent : undefined,
  }),
  component: () => <RunnersPage user={useUser()} toast={useToast()} />,
})
const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/settings',
  validateSearch: (s: Record<string, unknown>): { section?: string } => ({
    section: typeof s.section === 'string' ? s.section : undefined,
  }),
  component: SettingsScreen,
})
const inviteRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/invite',
  component: () => null,
})
const authorizationRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/authorize/device',
  component: () => null,
})
const instanceRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/instance',
  component: () => null,
})
const workspaceHomeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/workspaces',
  component: () => null,
})
const demoRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/demo',
  component: () => null,
})
const router = createRouter({
  basepath: workspaceBase || '/',
  routeTree: rootRoute.addChildren([
    momentsRoute,
    tasksRoute,
    dagRoute,
    runsRoute,
    runnersRoute,
    settingsRoute,
    inviteRoute,
    authorizationRoute,
    instanceRoute,
    workspaceHomeRoute,
    demoRoute,
  ]),
})
declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router
  }
}
createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <RouterProvider router={router} />
      </TooltipProvider>
    </QueryClientProvider>
  </React.StrictMode>,
)
