import { apiUrl } from '../api'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useState, useEffect, useRef } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Square, Redo2, RefreshCw, ClipboardCheck, Plus } from 'lucide-react'
import {
  api,
  terminal,
  stateNames,
  type Agent,
  type Run,
  type RunEvent,
  type User,
  type WorkspaceSettings,
} from '../api'
import { Button, IconButton, Field, Modal } from '../ui'
import { CopyValue } from './CopyValue'
import { RunResult, RunResultActions } from './RunResult'
import { RunDetails } from './RunDetails'
import { RunTrace, useRunTrace } from './RunTrace'
import { RunSection } from './RunSection'
import { RunLogs, RunLogActions } from './RunLogs'
import { ResumeRun } from '../sessions/ResumeRun'
import { PageActions } from '../PageHeader'
import { TaskEditor } from '../TaskEditor'
export function RunDetail({
  id,
  user,
  toast,
}: {
  id: string
  user: User
  toast: (message: string) => void
}) {
  useTranslation()
  const run = useQuery<Run>({
    queryKey: ['run', id],
    queryFn: () => api(`/runs/${id}`),
    refetchInterval: 2000,
  })
  const client = useQueryClient()
  const agents = useQuery<Agent[]>({
    queryKey: ['agents'],
    queryFn: () => api('/agents'),
    refetchInterval: 3000,
  })
  const [events, setEvents] = useState<RunEvent[]>([])
  const [tab, setTab] = useState<'execute' | 'details'>('execute')
  const [resultOpen, setResultOpen] = useState(false)
  const [raw, setRaw] = useState(false)
  const [processOpen, setProcessOpen] = useState(true)
  const [logsOpen, setLogsOpen] = useState(true)
  const [infoOpen, setInfoOpen] = useState(false)
  const [logError, setLogError] = useState(false)
  const [logAttempt, setLogAttempt] = useState(0)
  const highlighted = useRef(false)
  const execution = useRef<HTMLDivElement>(null)
  const hasResult = run.data?.result != null && run.data.result !== ''
  useEffect(() => {
    if (run.data?.state === 'succeeded' && hasResult && !highlighted.current) {
      highlighted.current = true
      setResultOpen(true)
      setProcessOpen(false)
      if (tab === 'execute') execution.current?.scrollTo({ top: 0 })
    }
  }, [run.data?.state, hasResult, tab])
  const trace = useRunTrace(id)
  const [follow, setFollow] = useState(true)
  const [resolve, setResolve] = useState(false)
  const [createTask, setCreateTask] = useState(false)
  const settings = useQuery<WorkspaceSettings>({
    queryKey: ['settings'],
    queryFn: () => api('/settings'),
    enabled: createTask,
  })
  useEffect(() => {
    if (createTask && settings.error) {
      toast(settings.error.message)
      setCreateTask(false)
    }
  }, [createTask, settings.error, toast])
  const navigate = useNavigate()
  const editable = user.role !== 'viewer'
  useEffect(() => {
    const source = new EventSource(apiUrl(`/runs/${id}/stream`))
    source.onopen = () => setLogError(false)
    source.onerror = () => setLogError(true)
    source.addEventListener('run_event', (e) => {
      const item = JSON.parse((e as MessageEvent).data) as RunEvent
      if (item.kind === 'trace' || item.kind === 'trace_status') trace.receive([item])
      if (item.kind !== 'trace')
        setEvents((old) =>
          old.some((v) => v.id === item.id)
            ? old
            : [...old, item].sort((a, b) => a.seq - b.seq).slice(-5000),
        )
      if (item.kind === 'state' || item.kind === 'snapshot')
        client.invalidateQueries({ queryKey: ['run', id] })
    })
    return () => source.close()
  }, [id, client, trace.receive, logAttempt])
  const action = useMutation({
    mutationFn: (type: string) => api<{ id?: string }>(`/runs/${id}/${type}`, 'POST', {}),
    onSuccess: (v) => {
      client.invalidateQueries({ queryKey: ['run', id] })
      if (v.id) navigate({ to: '/runs', search: { run: v.id } })
    },
    onError: (e) => toast(e.message),
  })
  if (!run.data)
    return (
      <div className="empty">
        <h3>{run.error ? tr('无法读取运行记录') : tr('正在加载…')}</h3>
        {run.error && (
          <>
            <p className="error">{run.error.message}</p>
            <Button onClick={() => run.refetch()}>
              <RefreshCw size={14} />
              {tr('重试')}
            </Button>
          </>
        )}
      </div>
    )
  const r = run.data
  const cancellable =
    r.state === 'queued' ||
    agents.data?.find(
      (a) =>
        a.id === r.config.agent &&
        (a.runner === r.config.runner || a.runner_id === r.config.runner),
    )?.cancellable
  const logs = events
    .filter((e) => ['log', 'truncated', 'progress'].includes(e.kind))
    .map((e) => e.data.text ?? e.data.message ?? '')
    .join('')
    .replace(/\x1b\[[0-9;]*m/g, '')
  return (
    <>
      <PageActions>
        <CopyValue label={tr('复制 Run ID')} value={id} toast={toast} />
        <ResumeRun
          run={r}
          id={id}
          available={terminal(r.state) && !!r.session?.ready}
          toast={toast}
        />
        {editable && terminal(r.state) && r.session?.ready && (
          <IconButton
            label={tr('从此会话创建任务')}
            loading={createTask && settings.isPending}
            onClick={() => setCreateTask(true)}
          >
            <Plus />
          </IconButton>
        )}
        {editable && ['queued', 'starting', 'running'].includes(r.state) && (
          <IconButton
            label={tr('取消')}
            tooltip={r.cancel_requested ? tr('取消已请求') : tr('取消执行')}
            danger
            disabled={!cancellable || r.cancel_requested || action.isPending}
            onClick={() => action.mutate('cancel')}
          >
            <Square />
          </IconButton>
        )}
        {editable && !r.task_archived && terminal(r.state) && (
          <IconButton
            label={tr('重试')}
            onClick={() => action.mutate('retry')}
            loading={action.isPending}
          >
            <Redo2 />
          </IconButton>
        )}
        {editable && r.state === 'unknown' && (
          <IconButton label={tr('核对状态')} onClick={() => setResolve(true)}>
            <ClipboardCheck />
          </IconButton>
        )}
      </PageActions>
      <div className="tabs" role="tablist" aria-label={tr('运行内容')}>
        {(
          [
            ['execute', tr('执行')],
            ['details', tr('详情')],
          ] as const
        ).map(([key, label]) => (
          <button
            key={key}
            role="tab"
            aria-selected={tab === key}
            className={tab === key ? 'active' : ''}
            onClick={() => setTab(key)}
          >
            {label}
          </button>
        ))}
      </div>
      <div className="run-page run-execution-page" ref={execution} hidden={tab !== 'execute'}>
        {['failed', 'timed_out', 'cancelled', 'interrupted', 'unknown', 'skipped'].includes(
          r.state,
        ) && (
          <div className="run-outcome content-card" role="status">
            {r.reason || stateNames[r.state] || r.state}
          </div>
        )}
        {['queued', 'starting'].includes(r.state) && (
          <p className="run-waiting quiet content-card">{tr('等待执行')}</p>
        )}
        {r.state === 'succeeded' && !hasResult && (
          <p className="run-waiting quiet content-card">{tr('未返回结果')}</p>
        )}
        <RunSection
          title={
            r.execution_snapshot?.trace_supported === false ||
            (!r.execution_snapshot?.trace_supported && !trace.state.nodes.size && !!logs)
              ? tr('输出')
              : tr('过程')
          }
          hidden={
            ['queued', 'starting', 'skipped'].includes(r.state) && !trace.state.nodes.size && !logs
          }
          open={processOpen}
          onChange={setProcessOpen}
          actions={
            r.execution_snapshot?.trace_supported === false ||
            (!r.execution_snapshot?.trace_supported && !trace.state.nodes.size && !!logs) ? (
              <RunLogActions text={logs} follow={follow} onFollow={setFollow} toast={toast} />
            ) : (
              <div className="trace-column-labels">
                <span>{tr('状态')}</span>
                <span>{tr('耗时')}</span>
              </div>
            )
          }
        >
          {r.execution_snapshot?.trace_supported === false ||
          (!r.execution_snapshot?.trace_supported && !trace.state.nodes.size && !!logs) ? (
            <RunLogs
              run={r}
              text={logs}
              follow={follow}
              visible={tab === 'execute' && processOpen}
              error={logError}
              retry={() => setLogAttempt((v) => v + 1)}
            />
          ) : (
            <RunTrace run={r} trace={trace} />
          )}
        </RunSection>
        <RunSection
          title={tr('结果')}
          open={resultOpen}
          onChange={setResultOpen}
          hidden={!hasResult}
          actions={<RunResultActions result={r.result} raw={raw} onRaw={setRaw} toast={toast} />}
        >
          <RunResult result={r.result} raw={raw} />
        </RunSection>
      </div>
      <div className="run-page run-diagnostic-page" hidden={tab !== 'details'}>
        <RunSection
          title={tr('日志')}
          open={logsOpen}
          onChange={setLogsOpen}
          actions={<RunLogActions text={logs} follow={follow} onFollow={setFollow} toast={toast} />}
        >
          <RunLogs
            run={r}
            text={logs}
            follow={follow}
            visible={tab === 'details' && logsOpen}
            error={logError}
            retry={() => setLogAttempt((v) => v + 1)}
          />
        </RunSection>
        <RunSection title={tr('执行信息')} open={infoOpen} onChange={setInfoOpen}>
          <RunDetails run={r} agents={agents.data} events={events} />
        </RunSection>
      </div>
      {resolve && <ResolveDialog id={id} onClose={() => setResolve(false)} />}
      {createTask && settings.data && r.session?.ready && (
        <TaskEditor
          defaultTimezone={settings.data.timezone}
          defaults={{
            runner: r.session.runner_id,
            agent: r.session.agent,
            cwd: r.session.cwd,
            parameters: r.execution_snapshot?.parameters ?? r.config.parameters,
            session: { id: r.session.id },
          }}
          onClose={() => setCreateTask(false)}
        />
      )}
    </>
  )
}
function ResolveDialog({ id, onClose }: { id: string; onClose: () => void }) {
  useTranslation()
  const [state, setState] = useState('interrupted')
  const [reason, setReason] = useState('')
  const [confirmed, setConfirmed] = useState(false)
  const [error, setError] = useState('')
  const client = useQueryClient()
  const save = useMutation({
    mutationFn: () => api(`/runs/${id}/resolve`, 'POST', { state, reason }),
    onSuccess: () => {
      client.invalidateQueries({ queryKey: ['run', id] })
      onClose()
    },
    onError: (e) => setError(e.message),
  })
  return (
    <Modal
      open
      onOpenChange={(v) => !v && onClose()}
      title={tr('核对执行状态')}
      description={tr('检查设备或外部任务后，为本次执行记录确认的结果。')}
    >
      <div className="modal-body">
        <Field label={tr('确认状态')}>
          <select value={state} onChange={(e) => setState(e.target.value)}>
            <option value="interrupted">{tr('已中断')}</option>
            <option value="cancelled">{tr('已取消')}</option>
            <option value="failed">{tr('失败')}</option>
            <option value="succeeded">{tr('成功')}</option>
          </select>
        </Field>
        <Field label={tr('核对说明')}>
          <textarea
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            placeholder={tr('记录核对方式与实际结果')}
            rows={3}
          />
        </Field>
        <label className="check">
          <input
            type="checkbox"
            checked={confirmed}
            onChange={(e) => setConfirmed(e.target.checked)}
          />{' '}
          {tr('我已核对实际任务状态')}
        </label>
        {error && <p className="error">{error}</p>}
      </div>
      <div className="modal-footer">
        <Button onClick={onClose}>{tr('取消')}</Button>
        <Button
          variant="solid"
          disabled={!confirmed || !reason.trim() || save.isPending}
          onClick={() => save.mutate()}
        >
          {tr('保存确认结果')}
        </Button>
      </div>
    </Modal>
  )
}
