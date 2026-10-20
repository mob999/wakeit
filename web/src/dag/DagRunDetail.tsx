import { ErrorToast } from '../Toast'
import { EditorMemoryContext, useEditorValue, type EditorMemory } from '../EditorMemory'
import { shortId } from './identity'
import { outputs } from './model'
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
  type Runner,
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
  const [memory, setMemory] = useState<Record<string, EditorMemory>>({})
  const runners = useQuery<Runner[]>({ queryKey: ['runners'], queryFn: () => api('/runners') })
  const [visited, setVisited] = useState(new Set([nodes[0]?.id]))
  const [tab, setTab] = useState<'execute' | 'details'>('execute')
  const [processOpen, setProcessOpen] = useState(true)
  const [resultOpen, setResultOpen] = useState(false)
  const [raw, setRaw] = useState(false)
  const highlighted = useRef(false)
  const emptyShown = useRef(false)
  const resultTop = useRef<HTMLDivElement>(null)
  const viewport = useRef<HTMLDivElement>(null)
  const client = useQueryClient()
  const navigate = useNavigate()
  const agents = useQuery<Agent[]>({ queryKey: ['agents'], queryFn: () => api('/agents') })
  function select(id: string) {
    setSelected(id)
    setVisited((old) => new Set([...old, id]))
  }
  const hasResult = r.result != null && r.result !== ''
  useEffect(() => {
    if (['succeeded', 'warning'].includes(r.state) && !hasResult && !emptyShown.current) {
      emptyShown.current = true
      setResultOpen(true)
      return
    }
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
        <div hidden={tab !== 'execute'}>
          <RunSection title={t('过程')} open={processOpen} onChange={setProcessOpen}>
            {!hasResult && r.reason && (
              <div className="dag-outcome" role="status">
                <span>{r.reason}</span>
                {['warning', 'failed', 'unknown'].includes(r.state) && (
                  <Button
                    onClick={() => {
                      const problem = nodes.find((n) =>
                        ['failed', 'timed_out', 'interrupted', 'skipped', 'unknown'].includes(
                          n.state,
                        ),
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

            <DagGraph
              compact
              hideTitle
              runners={runners.data}
              outputIds={outputs(r.config.dag!)}
              nodes={nodes}
              agents={agents.data}
              selected={selected}
              onSelect={select}
            />
            {nodes
              .filter((n) => visited.has(n.id))
              .map((n) => (
                <EditorMemoryContext.Provider
                  key={n.id}
                  value={{
                    value: memory[n.id] ?? {},
                    update: (update) =>
                      setMemory((previous) => ({
                        ...previous,
                        [n.id]:
                          typeof update === 'function' ? update(previous[n.id] ?? {}) : update,
                      })),
                  }}
                >
                  <NodeExecution
                    nodes={nodes}
                    onSelect={select}
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
                </EditorMemoryContext.Provider>
              ))}
          </RunSection>
          <div ref={resultTop} />
          <RunSection
            title={t('结果')}
            open={resultOpen}
            onChange={setResultOpen}
            hidden={!hasResult && !['succeeded', 'warning'].includes(r.state)}
            actions={<RunResultActions result={r.result} raw={raw} onRaw={setRaw} toast={toast} />}
          >
            {hasResult && r.reason && (
              <div className="dag-outcome" role="status">
                <span>{r.reason}</span>
                {['warning', 'failed', 'unknown'].includes(r.state) && (
                  <Button
                    onClick={() => {
                      const problem = nodes.find((n) =>
                        ['failed', 'timed_out', 'interrupted', 'skipped', 'unknown'].includes(
                          n.state,
                        ),
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

            {hasResult ? (
              <RunResult result={r.result} raw={raw} toast={toast} />
            ) : (
              <p className="quiet dag-node-empty">{t('未返回结果')}</p>
            )}
          </RunSection>
        </div>
        <div hidden={tab !== 'details'}>
          {nodes
            .filter((n) => visited.has(n.id))
            .map((n) => (
              <EditorMemoryContext.Provider
                key={n.id}
                value={{
                  value: memory[n.id] ?? {},
                  update: (update) =>
                    setMemory((previous) => ({
                      ...previous,
                      [n.id]: typeof update === 'function' ? update(previous[n.id] ?? {}) : update,
                    })),
                }}
              >
                <NodeExecution
                  nodes={nodes}
                  onSelect={select}
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
              </EditorMemoryContext.Provider>
            ))}
        </div>
      </div>
    </>
  )
}
function NodeExecution({
  nodes,
  onSelect,
  parent,
  node,
  name,
  state,
  reason,
  executionId: currentExecutionId,
  active,
  hidden,
  mode,
  user,
  agents,
  toast,
}: {
  nodes: DagRunNode[]
  onSelect: (id: string) => void
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
  const [chosen, setChosen] = useEditorValue<string | null>('attempt', null)
  const executionId = chosen ?? currentExecutionId
  const current = nodes.find((n) => n.id === node)
  const retry = useMutation({
    mutationFn: () =>
      api(`/runs/${parent}/nodes/${encodeURIComponent(node)}/retry`, 'POST', {
        execution_id: currentExecutionId,
      }),
    onSuccess: () => {
      client.invalidateQueries({ queryKey: ['run', parent] })
      client.invalidateQueries({ queryKey: ['dag-node', parent, node] })
    },
    onError: (error) => toast(error.message),
  })
  const execution = useQuery<{ execution: Run | null; attempts?: { id: string; state: string }[] }>(
    {
      queryKey: ['dag-node', parent, node, executionId],
      queryFn: () =>
        api(`/runs/${parent}/nodes/${encodeURIComponent(node)}?execution=${executionId}`),
      enabled: active && !!executionId,
      refetchInterval: active ? 2000 : false,
    },
  )
  const trace = useRunTrace(executionId ?? '', active && !!executionId && mode === 'trace')
  const [events, setEvents] = useState<RunEvent[]>([])
  const [logError, setLogError] = useState(false)
  const [attempt, setAttempt] = useState(0)
  const cursor = useRef(0)
  const [follow, setFollow] = useEditorValue(`follow:${executionId}`, true)
  const [logsOpen, setLogsOpen] = useEditorValue(`logsOpen:${executionId}`, true)
  const [infoOpen, setInfoOpen] = useEditorValue(`infoOpen:${executionId}`, false)
  const [resolve, setResolve] = useState(false)
  useEffect(() => {
    cursor.current = 0
    setEvents([])
    setLogError(false)
  }, [executionId])
  useEffect(() => {
    if (!active || !executionId) return
    let live = true
    const source = new EventSource(apiUrl(`/runs/${executionId}/stream?after=${cursor.current}`))
    source.onopen = () => setLogError(false)
    source.onerror = () => setLogError(true)
    source.addEventListener('run_event', (e) => {
      if (!live) return
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
    return () => {
      live = false
      source.close()
    }
  }, [active, executionId, mode, attempt, trace.receive, client, parent, node])
  const run = execution.data?.execution
  const displayedReason = chosen ? run?.reason : (run?.reason ?? reason)
  const startedAt = chosen ? run?.started_at : current?.started_at
  const finishedAt = chosen ? run?.finished_at : current?.finished_at
  const logs = events
    .filter((e) => ['log', 'truncated', 'progress'].includes(e.kind))
    .map((e) => e.data.text ?? e.data.message ?? '')
    .join('')
    .replace(/\x1b\[[0-9;]*m/g, '')
  const logActions = (
    <>
      <RunLogActions text={logs} follow={follow} onFollow={setFollow} toast={toast} />
      {logError && (
        <>
          <ErrorToast error={t('日志连接已中断，正在重连')} />
          <IconButton label={t('重新连接日志')} onClick={() => setAttempt((v) => v + 1)}>
            <RefreshCw />
          </IconButton>
        </>
      )}
    </>
  )
  return (
    <div
      hidden={hidden}
      aria-label={mode === 'details' ? t('dag.nodeDetails') : undefined}
      className={mode === 'details' ? 'content-card dag-node-details' : 'dag-node-process'}
    >
      <div className="dag-node-context">
        {nodes.length > 1 ? (
          <ChoicePicker
            label={t('dag.selectNode')}
            searchable
            value={node}
            options={nodes.map((n) => ({
              value: n.id,
              label: n.name,
              description: shortId(n.id),
              keywords: n.id,
              status: stateNames[n.state] ?? n.state,
            }))}
            onChange={onSelect}
          />
        ) : (
          <strong>{name}</strong>
        )}
        <Status state={execution.data?.execution?.state ?? state} />
        {(execution.data?.attempts?.length ?? 0) > 1 && (
          <div className="dag-attempt-picker">
            <ChoicePicker
              label={t('retry.attempts')}
              value={executionId ?? ''}
              options={execution.data!.attempts!.map((a, index) => ({
                value: a.id,
                label: t('retry.attempt', { number: index + 1 }),
                status: stateNames[a.state] ?? a.state,
              }))}
              onChange={(id) => setChosen(id === currentExecutionId ? null : id)}
            />
          </div>
        )}
        {!chosen && current?.retry_at && (
          <span className="quiet dag-retry-time">
            {t('retry.waiting')}{' '}
            <time dateTime={current.retry_at}>
              {new Date(current.retry_at).toLocaleTimeString()}
            </time>
          </span>
        )}
        {!chosen && current?.retry_at && user.role !== 'viewer' && (
          <IconButton
            label={t('retry.now')}
            loading={retry.isPending}
            onClick={() => retry.mutate()}
          >
            <Redo2 />
          </IconButton>
        )}
        {startedAt && finishedAt && (
          <span className="quiet mono">
            {Math.max(0, (Date.parse(finishedAt) - Date.parse(startedAt)) / 1000).toFixed(1)}s
          </span>
        )}
        {run && (
          <ResumeRun
            run={run}
            id={run.id}
            available={terminal(run.state) && !!run.session?.ready}
            toast={toast}
          />
        )}
        {!chosen && state === 'unknown' && user.role !== 'viewer' && (
          <IconButton label={t('核对状态')} onClick={() => setResolve(true)}>
            <ClipboardCheck />
          </IconButton>
        )}
      </div>
      {displayedReason && executionId && <p className="dag-node-reason">{displayedReason}</p>}
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
              {logActions}
            </div>
            <RunLogs
              run={run}
              text={logs}
              follow={follow}
              visible={active}
              error={false}
              retry={() => setAttempt((v) => v + 1)}
            />
          </>
        ) : (
          <RunTrace run={run} trace={trace} />
        )
      ) : (
        <>
          <RunSection
            embedded
            title={t('日志')}
            open={logsOpen}
            onChange={setLogsOpen}
            actions={logActions}
          >
            <RunLogs
              run={run}
              text={logs}
              follow={follow}
              visible={active && logsOpen}
              error={false}
              retry={() => setAttempt((v) => v + 1)}
            />
          </RunSection>
          {chosen && terminal(run.state) && run.result != null && (
            <RunSection embedded title={t('结果')} open onChange={() => {}}>
              <RunResult result={run.result} raw={false} toast={toast} />
            </RunSection>
          )}
          <RunSection embedded title={t('执行信息')} open={infoOpen} onChange={setInfoOpen}>
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
