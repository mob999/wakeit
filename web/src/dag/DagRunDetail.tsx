import { ErrorToast } from '../Toast'
import { useEffect, useRef, useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Square, Redo2, ClipboardCheck, RefreshCw } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { t } from '../i18n'
import {
  api,
  apiUrl,
  terminal,
  stateNames,
  type Run,
  type RunEvent,
  type DagRunNode,
  type User,
  type Agent,
} from '../api'
import { IconButton, Button } from '../ui'
import { PageActions } from '../PageHeader'
import { Status } from '../Status'
import { ChoicePicker } from '../ChoicePicker'
import { CopyValue } from '../runs/CopyValue'
import { RunSection } from '../runs/RunSection'
import { RunResult, RunResultActions } from '../runs/RunResult'
import { RunTrace, useRunTrace } from '../runs/RunTrace'
import { RunLogs, RunLogActions } from '../runs/RunLogs'
import { RunDetails } from '../runs/RunDetails'
import { ResolveDialog } from '../runs/RunDetail'
import { ResumeRun } from '../sessions/ResumeRun'
import { DagGraph } from './DagGraph'

export function DagRunDetail({
  id,
  run: r,
  user,
  toast,
}: {
  id: string
  run: Run
  user: User
  toast: (s: string) => void
}) {
  useTranslation()
  const nodes: DagRunNode[] =
    r.nodes ?? r.config.dag!.nodes.map((n) => ({ ...n, state: 'waiting' }))
  const [selected, setSelected] = useState(nodes[0]?.id)
  const [visited, setVisited] = useState(new Set([nodes[0]?.id]))
  const [tab, setTab] = useState<'execute' | 'details'>('execute')
  const [processOpen, setProcessOpen] = useState(true)
  const [resultOpen, setResultOpen] = useState(false)
  const [raw, setRaw] = useState(false)
  const highlighted = useRef(false)
  const resultTop = useRef<HTMLDivElement>(null)
  const viewport = useRef<HTMLDivElement>(null)
  const client = useQueryClient()
  const navigate = useNavigate()
  const agents = useQuery<Agent[]>({ queryKey: ['agents'], queryFn: () => api('/agents') })
  function select(id: string) {
    setSelected(id)
    setVisited((old) => new Set([...old, id]))
  }
  const hasResult = r.result != null
  useEffect(() => {
    if (['succeeded', 'warning'].includes(r.state) && hasResult && !highlighted.current) {
      highlighted.current = true
      setProcessOpen(false)
      setResultOpen(true)
      if (tab === 'execute')
        requestAnimationFrame(() => {
          const pane = viewport.current,
            target = resultTop.current
          if (pane && target) pane.scrollTo({ top: target.offsetTop - pane.offsetTop })
        })
    }
  }, [r.state, hasResult, tab])
  const action = useMutation({
    mutationFn: (type: string) => api<{ id?: string }>(`/runs/${id}/${type}`, 'POST', {}),
    onSuccess: (v) => {
      client.invalidateQueries({ queryKey: ['run', id] })
      if (v.id) navigate({ to: '/runs', search: { run: v.id } })
    },
    onError: (e) => toast(e.message),
  })
  return (
    <>
      <PageActions>
        <CopyValue label={t('复制 Run ID')} value={id} toast={toast} />
        <ResumeRun
          run={r}
          id={id}
          available={terminal(r.state) && nodes.some((n) => n.continuation_available)}
          toast={toast}
        />
        {user.role !== 'viewer' && !terminal(r.state) && (
          <IconButton
            label={t('取消执行')}
            danger
            disabled={r.cancel_requested}
            loading={action.isPending}
            onClick={() => action.mutate('cancel')}
          >
            <Square />
          </IconButton>
        )}
        {user.role !== 'viewer' && terminal(r.state) && !r.task_archived && (
          <IconButton
            label={t('重试')}
            loading={action.isPending}
            onClick={() => action.mutate('retry')}
          >
            <Redo2 />
          </IconButton>
        )}
      </PageActions>
      <div className="tabs" role="tablist" aria-label={t('运行内容')}>
        {(['execute', 'details'] as const).map((key) => (
          <button
            key={key}
            role="tab"
            aria-selected={tab === key}
            className={tab === key ? 'active' : ''}
            onClick={() => setTab(key)}
          >
            {t(key === 'execute' ? '执行' : '详情')}
          </button>
        ))}
      </div>
      <div className="run-page dag-run-page" ref={viewport}>
        {r.reason && (
          <div
            className={`content-card dag-outcome ${r.state === 'warning' ? 'dag-warning' : ''}`}
            role="status"
          >
            <span>{r.reason}</span>
            {['warning', 'failed', 'unknown'].includes(r.state) && (
              <Button
                onClick={() => {
                  const problem = nodes.find((n) =>
                    ['failed', 'timed_out', 'interrupted', 'skipped', 'unknown'].includes(n.state),
                  )
                  if (problem) select(problem.id)
                  setTab('execute')
                  setProcessOpen(true)
                }}
              >
                {t('dag.problemNodes')}
              </Button>
            )}
          </div>
        )}
        {tab === 'details' && (
          <div className="dag-selected-caption content-card">
            <ChoicePicker
              label={t('dag.selectNode')}
              value={selected ?? ''}
              options={nodes.map((n) => ({
                value: n.id,
                label: n.name,
                status: stateNames[n.state] ?? n.state,
              }))}
              onChange={select}
            />
          </div>
        )}
        <div hidden={tab !== 'execute'}>
          <RunSection title={t('过程')} open={processOpen} onChange={setProcessOpen}>
            <DagGraph
              compact
              nodes={nodes}
              agents={agents.data}
              selected={selected}
              onSelect={select}
            />
            {nodes
              .filter((n) => visited.has(n.id))
              .map((n) => (
                <NodeExecution
                  key={n.id}
                  parent={id}
                  node={n.id}
                  name={n.name}
                  state={n.state}
                  reason={n.reason}
                  executionId={n.execution_id}
                  active={selected === n.id && processOpen && tab === 'execute'}
                  hidden={selected !== n.id}
                  mode="trace"
                  user={user}
                  toast={toast}
                  agents={agents.data}
                />
              ))}
          </RunSection>
          <div ref={resultTop} />
          <RunSection
            title={t('结果')}
            open={resultOpen}
            onChange={setResultOpen}
            hidden={!hasResult}
            actions={<RunResultActions result={r.result} raw={raw} onRaw={setRaw} toast={toast} />}
          >
            <RunResult result={r.result} raw={raw} toast={toast} />
          </RunSection>
          {!hasResult && terminal(r.state) && (
            <p className="quiet content-card run-waiting">{t('未返回结果')}</p>
          )}
        </div>
        <div hidden={tab !== 'details'}>
          {nodes
            .filter((n) => visited.has(n.id))
            .map((n) => (
              <NodeExecution
                key={n.id}
                parent={id}
                node={n.id}
                name={n.name}
                state={n.state}
                reason={n.reason}
                executionId={n.execution_id}
                active={selected === n.id && tab === 'details'}
                hidden={selected !== n.id}
                mode="details"
                user={user}
                toast={toast}
                agents={agents.data}
              />
            ))}
        </div>
      </div>
    </>
  )
}
function NodeExecution({
  parent,
  node,
  name,
  state,
  reason,
  executionId,
  active,
  hidden,
  mode,
  user,
  agents,
  toast,
}: {
  parent: string
  node: string
  name: string
  state: string
  reason?: string | null
  executionId?: string | null
  active: boolean
  hidden: boolean
  mode: 'trace' | 'details'
  user: User
  agents?: Agent[]
  toast: (s: string) => void
}) {
  const client = useQueryClient()
  const execution = useQuery<{ execution: Run | null }>({
    queryKey: ['dag-node', parent, node],
    queryFn: () => api(`/runs/${parent}/nodes/${encodeURIComponent(node)}`),
    enabled: active && !!executionId,
    refetchInterval: active ? 2000 : false,
  })
  const trace = useRunTrace(executionId ?? '', active && !!executionId && mode === 'trace')
  const [events, setEvents] = useState<RunEvent[]>([])
  const [logError, setLogError] = useState(false)
  const [attempt, setAttempt] = useState(0)
  const cursor = useRef(0)
  const [follow, setFollow] = useState(true)
  const [logsOpen, setLogsOpen] = useState(true)
  const [infoOpen, setInfoOpen] = useState(false)
  const [resolve, setResolve] = useState(false)
  useEffect(() => {
    if (!active || !executionId) return
    const source = new EventSource(apiUrl(`/runs/${executionId}/stream?after=${cursor.current}`))
    source.onopen = () => setLogError(false)
    source.onerror = () => setLogError(true)
    source.addEventListener('run_event', (e) => {
      const item = JSON.parse((e as MessageEvent).data) as RunEvent
      cursor.current = Math.max(cursor.current, item.id)
      if (mode === 'trace') trace.receive([item])
      if (item.kind !== 'trace')
        setEvents((old) => (old.some((v) => v.id === item.id) ? old : [...old, item].slice(-5000)))
      if (['state', 'node_state'].includes(item.kind)) {
        client.invalidateQueries({ queryKey: ['dag-node', parent, node] })
        client.invalidateQueries({ queryKey: ['run', parent] })
      }
    })
    return () => source.close()
  }, [active, executionId, mode, attempt, trace.receive, client, parent, node])
  const run = execution.data?.execution
  const logs = events
    .filter((e) => ['log', 'truncated', 'progress'].includes(e.kind))
    .map((e) => e.data.text ?? e.data.message ?? '')
    .join('')
    .replace(/\x1b\[[0-9;]*m/g, '')
  return (
    <div hidden={hidden}>
      <div className="dag-selected-caption">
        <strong>{name}</strong>
        <Status state={state} />
        {run && (
          <ResumeRun
            run={run}
            id={run.id}
            available={terminal(run.state) && !!run.session?.ready}
            toast={toast}
          />
        )}
        {state === 'unknown' && user.role !== 'viewer' && (
          <IconButton label={t('核对状态')} onClick={() => setResolve(true)}>
            <ClipboardCheck />
          </IconButton>
        )}
      </div>
      {!executionId ? (
        <p className="quiet dag-node-empty">
          {reason || stateNames[state] || t('dag.noExecution')}
        </p>
      ) : execution.error ? (
        <ErrorToast>
          {execution.error.message}
          <Button onClick={() => execution.refetch()}>
            <RefreshCw size={14} />
            {t('重试')}
          </Button>
        </ErrorToast>
      ) : !run ? (
        <p className="quiet dag-node-empty">{t('正在加载…')}</p>
      ) : mode === 'trace' ? (
        run.execution_snapshot?.trace_supported === false ? (
          <>
            <div className="dag-selected-caption">
              <span>{t('输出')}</span>
              <RunLogActions text={logs} follow={follow} onFollow={setFollow} toast={toast} />
            </div>
            <RunLogs
              run={run}
              text={logs}
              follow={follow}
              visible={active}
              error={logError}
              retry={() => setAttempt((v) => v + 1)}
            />
          </>
        ) : (
          <RunTrace run={run} trace={trace} />
        )
      ) : (
        <>
          <RunSection
            title={t('日志')}
            open={logsOpen}
            onChange={setLogsOpen}
            actions={
              <RunLogActions text={logs} follow={follow} onFollow={setFollow} toast={toast} />
            }
          >
            <RunLogs
              run={run}
              text={logs}
              follow={follow}
              visible={active && logsOpen}
              error={logError}
              retry={() => setAttempt((v) => v + 1)}
            />
          </RunSection>
          <RunSection title={t('执行信息')} open={infoOpen} onChange={setInfoOpen}>
            <RunDetails run={run} agents={agents} events={events} />
          </RunSection>
        </>
      )}
      {resolve && (
        <ResolveDialog
          id={parent}
          endpoint={`/runs/${parent}/nodes/${encodeURIComponent(node)}/resolve`}
          onClose={() => setResolve(false)}
        />
      )}
    </div>
  )
}
