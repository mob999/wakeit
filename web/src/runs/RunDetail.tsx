import { ActionIcon } from '../actions/ActionIcon'
import { ErrorToast } from '../components/Toast'
import { apiUrl } from '../api/client'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useState, useEffect, useRef } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { RefreshCw } from '../components/icons'
import { api } from '../api/client'
import { terminal } from './state'
import { type Agent, type Run, type RunEvent, type User } from '../api/types'
import { Button, IconButton, Field, Modal } from '../components/ui'
import { RunResult, RunResultActions } from './RunResult'
import { RunDetails } from './RunDetails'
import { RunTrace, useRunTrace } from './RunTrace'
import { RunSection } from './RunSection'
import { RunLogs, RunLogActions } from './RunLogs'
import { RunActions } from './RunActions'
import { DagRunDetail } from '../dag/DagRunDetail'
import { TraceActions } from './RunReading'
import { RunTabs, RunPanel, useRunView } from './RunTabs'
import { RunReason } from './RunReason'
import { Status } from '../components/Status'
function SingleRunDetail({
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
  const [raw, setRaw] = useState(false)
  const [logsOpen, setLogsOpen] = useState(true)
  const [infoOpen, setInfoOpen] = useState(false)
  const [logError, setLogError] = useState(false)
  const [logAttempt, setLogAttempt] = useState(0)
  const execution = useRef<HTMLDivElement>(null)
  const hasResult = run.data?.result != null && run.data.result !== ''
  const { reading, updateReading, tab, setTab } = useRunView(
    `execution:${id}`,
    hasResult || run.data?.state === 'succeeded',
    !!run.data,
  )
  const trace = useRunTrace(id)
  const [follow, setFollow] = useState(false)
  useEffect(() => {
    if (run.data?.execution_snapshot?.trace_supported === false) updateReading({ follow })
  }, [run.data?.execution_snapshot?.trace_supported, follow, updateReading])
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
  if (!run.data)
    return (
      <div className="empty">
        <h3>{run.error ? tr('无法读取运行记录') : tr('正在加载…')}</h3>
        {run.error && (
          <>
            <ErrorToast error={run.error.message} />
            <Button onClick={() => run.refetch()}>
              <RefreshCw size={14} />
              {tr('重试')}
            </Button>
          </>
        )}
      </div>
    )
  const r = run.data
  const logs = events
    .filter((e) => ['log', 'truncated', 'progress'].includes(e.kind))
    .map((e) => e.data.text ?? e.data.message ?? '')
    .join('')
    .replace(/\x1b\[[0-9;]*m/g, '')
  return (
    <>
      <RunActions run={r} user={user} agents={agents.data} toast={toast} />
      <RunTabs id={id} tab={tab} onChange={setTab} outputReady={reading.resultReady} />
      <RunPanel id={id} tab="output" active={tab} className="run-output-page">
        <RunSection
          title={tr('run.output')}
          collapsible={false}
          actions={<RunResultActions result={r.result} raw={raw} onRaw={setRaw} toast={toast} />}
        >
          {hasResult ? (
            <RunResult result={r.result} raw={raw} source={{ runId: r.id }} cwd={r.config.cwd} />
          ) : (
            <p className="run-waiting quiet text-muted">
              {tr(terminal(r.state) ? '未返回结果' : '等待执行输出…')}
            </p>
          )}
        </RunSection>
      </RunPanel>
      <RunPanel id={id} tab="trace" active={tab} className="run-execution-page" ref={execution}>
        {['queued', 'starting'].includes(r.state) && (
          <p className="run-waiting quiet text-muted content-card">{tr('等待执行')}</p>
        )}
        <RunSection
          title={tr('run.trace')}
          collapsible={false}
          actions={
            <>
              {reading.resultReady && (
                <button
                  type="button"
                  className="result-ready-inline"
                  onClick={() => setTab('output')}
                >
                  {tr('reading.resultReady')}
                </button>
              )}

              {r.execution_snapshot?.trace_supported === false ||
              (!r.execution_snapshot?.trace_supported && !trace.state.nodes.size && !!logs) ? (
                <RunLogActions text={logs} follow={follow} onFollow={setFollow} toast={toast} />
              ) : (
                <TraceActions id={id} issues={trace.state.issues} />
              )}
            </>
          }
        >
          {r.reason && (
            <div className="run-context">
              <RunReason key={r.id} state={r.state} reason={r.reason}>
                <Status state={r.state} />
              </RunReason>
            </div>
          )}
          {r.execution_snapshot?.trace_supported === false ||
          (!r.execution_snapshot?.trace_supported && !trace.state.nodes.size && !!logs) ? (
            <RunLogs
              run={r}
              text={logs}
              follow={follow}
              onFollowChange={setFollow}
              visible={tab === 'trace'}
              error={logError}
              retry={() => setLogAttempt((v) => v + 1)}
            />
          ) : (
            <RunTrace
              key={r.id}
              run={r}
              trace={trace}
              viewport={execution}
              active={tab === 'trace'}
            />
          )}
        </RunSection>
      </RunPanel>
      <RunPanel id={id} tab="details" active={tab} className="run-diagnostic-page">
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
            onFollowChange={setFollow}
            visible={tab === 'details' && logsOpen}
            error={logError}
            retry={() => setLogAttempt((v) => v + 1)}
          />
        </RunSection>
        <RunSection title={tr('执行信息')} open={infoOpen} onChange={setInfoOpen}>
          <RunDetails run={r} agents={agents.data} events={events} />
        </RunSection>
      </RunPanel>
    </>
  )
}
export function ResolveDialog({
  id,
  onClose,
  endpoint,
}: {
  id: string
  onClose: () => void
  endpoint?: string
}) {
  useTranslation()
  const [state, setState] = useState('interrupted')
  const [reason, setReason] = useState('')
  const [confirmed, setConfirmed] = useState(false)
  const [error, setError] = useState('')
  const client = useQueryClient()
  const save = useMutation({
    mutationFn: () => api(endpoint ?? `/runs/${id}/resolve`, 'POST', { state, reason }),
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
        {error && <ErrorToast error={error} />}
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

export function RunDetail(props: { id: string; user: User; toast: (message: string) => void }) {
  const run = useQuery<Run>({
    queryKey: ['run', props.id],
    queryFn: () => api(`/runs/${props.id}`),
    refetchInterval: 2000,
  })
  if (run.data?.config.dag) return <DagRunDetail key={props.id} {...props} run={run.data} />
  return <SingleRunDetail key={props.id} {...props} />
}
