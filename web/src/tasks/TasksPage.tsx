import { ActionMenu, ActionMenuItem, CopyMenuItem } from '../actions/ActionMenu'
import { ActionIcon } from '../actions/ActionIcon'
import { useToast, ErrorToast } from '../components/Toast'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { ContentCard } from '../components/ContentCard'
import { DetailTabs } from '../components/DetailTabs'
import { Button, IconButton, Modal } from '../components/ui'
import { CopyValue } from '../runs/CopyValue'
import { PageActions } from '../components/PageHeader'
import { TaskEditor } from './TaskEditor'
import { CreateTaskDialog } from './CreateTaskDialog'
import { TaskOverview } from './TaskOverview'
import { RunTable } from '../runs/RunTable'
import { Split } from '../components/Workbench'
import { Status } from '../components/Status'
import { RunIdentity } from '../runs/RunListRow'
import { useFilters } from '../search/context'
import { listPath } from '../search/filters'
import { type PageResult } from '../api/types'
import { Pagination } from '../components/Pagination'
import { useState, useEffect } from 'react'
import { useQuery, useMutation } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { ChevronRight, Clock } from '../components/icons'
import YAML from 'yaml'
import { api } from '../api/client'
import { type User, type Task, type Run } from '../api/types'
import { time } from '../i18n'
import { useQueryClient } from '@tanstack/react-query'
import { Empty } from '../components/Empty'
import { useAgents } from '../runners/useAgents'

function CopyButton({ value }: { value: string }) {
  useTranslation()
  return <CopyValue value={value} toast={useToast()} />
}

export function TasksPage({
  user: u,
  search: routeSearch,
}: {
  user: User
  search: { task?: string }
}) {
  useTranslation()
  const agents = useAgents()
  const client = useQueryClient()
  const filters = useFilters('tasks')
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
  const [tab, setTab] = useState<'overview' | 'runs' | 'config'>('overview')
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
          {archive.error && <ErrorToast error={archive.error.message} />}
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
                <IconButton
                  label={tr('新建任务')}
                  disabled={!settings.data}
                  onClick={() => setEditor('new')}
                >
                  <ActionIcon name="add" />
                </IconButton>
              )}
            </div>
            <div className="list-scroll">
              {tasks.error ? (
                <ErrorToast error={tasks.error.message} />
              ) : tasks.isPending ? (
                <p className="quiet text-muted padded p-5">{tr('正在加载…')}</p>
              ) : tasks.data?.total === 0 ? (
                <Empty title={tr('暂无任务')} />
              ) : (
                tasks.data?.items.map((t) => (
                  <button
                    key={t.id}
                    className={`task-row ${task?.id === t.id ? 'active' : ''}`}
                    aria-current={task?.id === t.id ? 'page' : undefined}
                    onClick={() => {
                      setSelected(t.id)
                      navigate({ to: '/tasks', search: { task: t.id } })
                      setTab('overview')
                    }}
                  >
                    <div className="row-title">
                      <span title={t.config.name}>{t.config.name}</span>
                      <Status state={t.last_run?.state ?? 'not_run'} />
                    </div>
                    <div className="row-meta">
                      <RunIdentity run={t} agents={agents.data} />
                    </div>
                    <div className="row-meta next">
                      <Clock size={12} />
                      {t.config.enabled ? time(t.next_at) : tr('actions.taskDisabled')}
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
          <Empty title={tr('暂无任务')}></Empty>
        ) : (
          <>
            <PageActions>
              {task.archived_at && <span className="quiet text-muted">{tr('已归档')}</span>}
              {editable && !task.archived_at && (
                <>
                  <div
                    className="task-control-group"
                    role="group"
                    aria-label={tr('actions.taskControls')}
                  >
                    <IconButton
                      label={tr('立即执行')}
                      className="task-control-run"
                      disabled={action.isPending}
                      loading={action.isPending && action.variables?.kind === 'trigger'}
                      onClick={() => action.mutate({ task, kind: 'trigger' })}
                    >
                      <ActionIcon name="play" />
                    </IconButton>
                    <IconButton
                      label={tr(task.config.enabled ? 'actions.disableTask' : 'actions.enableTask')}
                      className={`task-control-schedule ${task.config.enabled ? 'is-enabled' : 'is-disabled'}`}
                      disabled={action.isPending || !task.config.schedule}
                      disabledReason={!task.config.schedule ? tr('actions.noSchedule') : undefined}
                      loading={action.isPending && action.variables?.kind === 'toggle'}
                      onClick={() => action.mutate({ task, kind: 'toggle' })}
                    >
                      <ActionIcon name={task.config.enabled ? 'disableTask' : 'enableTask'} />
                    </IconButton>
                    <IconButton
                      label={tr('actions.editTask')}
                      className="task-control-edit"
                      onClick={() =>
                        task.config.dag
                          ? navigate({ to: '/tasks/dag', search: { task: task.id } })
                          : setEditor(task)
                      }
                    >
                      <ActionIcon name="edit" />
                    </IconButton>
                  </div>
                </>
              )}
              <div className="action-group" role="group" aria-label={tr('actions.taskEditing')}>
                <ActionMenu label={tr('actions.taskMenu')}>
                  <CopyMenuItem label={tr('复制 Task ID')} value={task.id} toast={toast} />
                  {editable && !task.archived_at && (
                    <>
                      <ActionMenuItem
                        icon="archive"
                        disabled={action.isPending}
                        onSelect={() => {
                          archive.reset()
                          setArchiving(task)
                        }}
                      >
                        {tr('task.archive')}
                      </ActionMenuItem>
                    </>
                  )}
                </ActionMenu>
              </div>
            </PageActions>
            <DetailTabs
              className="task-detail-tabs"
              idPrefix={`task-${task.id}`}
              label={tr('task.content')}
              tabs={[
                { key: 'overview', label: tr('概览') },
                { key: 'runs', label: tr('执行历史') },
                { key: 'config', label: tr('配置') },
              ]}
              value={tab}
              onChange={setTab}
            />
            <div
              className="detail-scroll"
              id={`task-${task.id}-${tab}-panel`}
              role="tabpanel"
              aria-labelledby={`task-${task.id}-${tab}-tab`}
              tabIndex={0}
            >
              {tab === 'overview' ? (
                <TaskOverview
                  key={task.id}
                  task={task}
                  agents={agents.data}
                  runs={history.data?.items.slice(0, 5) ?? []}
                  pending={history.isPending}
                  error={history.error}
                  onHistory={() => setTab('runs')}
                  toast={toast}
                />
              ) : tab === 'runs' ? (
                <>
                  <RunTable
                    title={tr('执行历史')}
                    runs={history.data?.items ?? []}
                    agents={agents.data}
                    pending={history.isPending}
                    error={history.error}
                  />
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
      {editor === 'new' ? (
        <CreateTaskDialog
          timezone={settings.data?.timezone ?? 'Asia/Shanghai'}
          onClose={() => setEditor(null)}
          onCreated={(id, runId) =>
            runId
              ? navigate({ to: '/runs', search: { run: runId } })
              : navigate({ to: '/tasks', search: { task: id } })
          }
        />
      ) : (
        editor && (
          <TaskEditor
            task={editor}
            defaultTimezone={settings.data?.timezone ?? 'Asia/Shanghai'}
            onClose={() => setEditor(null)}
          />
        )
      )}
    </>
  )
}
