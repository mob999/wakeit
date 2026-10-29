import { ActionIcon } from '../actions/ActionIcon'
import { ErrorToast } from '../Toast'
import { EditorMemoryContext, useEditorValue, type EditorMemory } from '../EditorMemory'
import { shortId } from './identity'
import { outputs } from './model'
import { useEffect, useRef, useState, useCallback, type RefObject, type ReactNode } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { RefreshCw } from '../icons'
import { useTranslation } from 'react-i18next'
import { t, locale } from '../i18n'
import { apiErrorText } from '../apiErrorText'
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
import { RunActions } from '../runs/RunActions'
import { Status } from '../Status'
import { ChoicePicker } from '../ChoicePicker'
import { RunSection } from '../runs/RunSection'
import { RunResult } from '../runs/RunResult'
import { RunTrace, useRunTrace } from '../runs/RunTrace'
import { RunLogs, RunLogActions } from '../runs/RunLogs'
import { RunDetails } from '../runs/RunDetails'
import { ResolveDialog } from '../runs/RunDetail'
import { ResumeRun } from '../sessions/ResumeRun'
import { DagGraph, type DagView } from './DagGraph'
import { TraceActions } from '../runs/RunReading'
import { RunTabs, RunPanel, useRunView } from '../runs/RunTabs'
import { RunReason } from '../runs/RunReason'
import { DagRunStatus } from './DagRunStatus'
import { DagRunResult } from './DagRunResult'

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
  const { reading, updateReading, tab, setTab } = useRunView(
    `dag:${id}`,
    (r.result != null && r.result !== '') || ['succeeded', 'warning'].includes(r.state),
  )
  const [structureOpen, setStructureOpen] = useState(true)
  const [view, setView] = useState<DagView>('graph')
  const [search, setSearch] = useState('')
  const [selected, setSelected] = useState(nodes[0]?.id)
  const [memory, setMemory] = useState<Record<string, EditorMemory>>({})
  const runners = useQuery<Runner[]>({ queryKey: ['runners'], queryFn: () => api('/runners') })
  const [visited, setVisited] = useState(new Set([nodes[0]?.id]))
  const viewport = useRef<HTMLDivElement>(null)
  const agents = useQuery<Agent[]>({ queryKey: ['agents'], queryFn: () => api('/agents') })
  function select(id: string, focus = false) {
    setSelected(id)
    setVisited((old) => new Set([...old, id]))
    requestAnimationFrame(() => {
      const context = viewport.current?.querySelector<HTMLElement>(
        '.dag-node-process:not([hidden]) > .dag-node-context',
      )
      if (!context || !viewport.current) return
      const rect = context.getBoundingClientRect(),
        pane = viewport.current.getBoundingClientRect()
      if (rect.top < pane.top || rect.bottom > pane.bottom)
        context.scrollIntoView({ block: 'nearest' })
      if (focus) context.focus({ preventScroll: true })
    })
  }
  function selectIssue(id: string) {
    setMemory((previous) => ({ ...previous, [id]: { ...previous[id], attempt: null } }))
    setTab('trace')
    select(id, true)
  }
  const setFollowing = useCallback(
    (follow: boolean) => updateReading((old) => (old.follow === follow ? old : { ...old, follow })),
    [updateReading],
  )
  const resultAction = reading.resultReady ? (
    <button type="button" className="result-ready-inline" onClick={() => setTab('output')}>
      {t('reading.resultReady')}
    </button>
  ) : null
  return (
    <>
      <RunActions run={{ ...r, nodes }} user={user} agents={agents.data} toast={toast} />
      <RunTabs
        id={id}
        tab={tab}
        onChange={setTab}
        outputReady={reading.resultReady}
        status={<DagRunStatus run={r} nodes={nodes} onSelect={selectIssue} />}
      />
      <RunPanel id={id} tab="output" active={tab} className="run-output-page">
        <DagRunResult
          key={id}
          runId={id}
          result={r.result}
          nodes={nodes}
          pending={!terminal(r.state)}
          toast={toast}
        />
      </RunPanel>
      <RunPanel id={id} tab="trace" active={tab} className="dag-run-page" ref={viewport}>
        <RunSection
          title={t('run.trace')}
          collapsible={false}
          actions={
            <div className="dag-reading-tools">
              <IconButton
                label={t('dag.graph')}
                aria-pressed={view === 'graph'}
                onClick={() => {
                  setView('graph')
                  setStructureOpen(true)
                }}
              >
                <ActionIcon name="graph" />
              </IconButton>
              <IconButton
                label={t('dag.list')}
                aria-pressed={view === 'list'}
                onClick={() => {
                  setView('list')
                  setStructureOpen(true)
                }}
              >
                <ActionIcon name="list" />
              </IconButton>
              {structureOpen && (
                <input
                  aria-label={t('dag.search')}
                  placeholder={t('dag.search')}
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                />
              )}
              <IconButton
                label={t(structureOpen ? 'reading.hideStructure' : 'reading.showStructure')}
                aria-expanded={structureOpen}
                onClick={() => setStructureOpen((v) => !v)}
              >
                {structureOpen ? (
                  <ActionIcon name="hideStructure" />
                ) : (
                  <ActionIcon name="showStructure" />
                )}
              </IconButton>
            </div>
          }
        >
          <div className="dag-run-structure" hidden={!structureOpen}>
            <DagGraph
              compact
              hideHeader
              view={view}
              onViewChange={setView}
              search={search}
              onSearchChange={setSearch}
              runners={runners.data}
              outputIds={outputs(r.config.dag!)}
              nodes={nodes}
              agents={agents.data}
              selected={selected}
              onSelect={select}
            />
          </div>
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
                  active={selected === n.id && tab === 'trace'}
                  hidden={selected !== n.id}
                  mode="trace"
                  viewport={viewport}
                  onFollowChange={setFollowing}
                  resultAction={resultAction}
                  user={user}
                  toast={toast}
                  agents={agents.data}
                />
              </EditorMemoryContext.Provider>
            ))}
        </RunSection>
      </RunPanel>
      <RunPanel id={id} tab="details" active={tab} className="dag-run-page">
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
      </RunPanel>
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
  viewport,
  onFollowChange,
  resultAction,
}: {
  resultAction?: ReactNode
  viewport?: RefObject<HTMLDivElement | null>
  onFollowChange?: (follow: boolean) => void
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
    mutationFn: (resumeStopped: boolean) =>
      api(`/runs/${parent}/nodes/${encodeURIComponent(node)}/retry`, 'POST', {
        execution_id: currentExecutionId ?? null,
        resume_stopped: resumeStopped,
      }),
    onSuccess: () => {
      client.invalidateQueries({ queryKey: ['runs'] })
      client.invalidateQueries({ queryKey: ['moments'] })
      client.invalidateQueries({ queryKey: ['search'] })
      client.invalidateQueries({ queryKey: ['run', parent] })
      client.invalidateQueries({ queryKey: ['dag-node', parent, node] })
    },
    onError: (error) => toast(error.message),
  })
  const execution = useQuery<{
    execution: Run | null
    attempts?: { id: string; state: string }[]
    retry?: {
      action: 'retry' | 'accelerate' | 'resume' | null
      reason?: string
      affected: { id: string; name: string; state: string }[]
    }
  }>({
    queryKey: ['dag-node', parent, node, executionId],
    queryFn: () =>
      api(
        `/runs/${parent}/nodes/${encodeURIComponent(node)}${executionId ? `?execution=${executionId}` : ''}`,
      ),
    enabled: active,
    refetchInterval: active ? 2000 : false,
  })
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
  const pickerId = (id: string) => `dag-node-picker-${parent}-${mode}-${id}`
  const displayedReason = run?.reason ?? (!executionId ? reason : undefined)
  const startedAt = run?.started_at
  const finishedAt = run?.finished_at
  useEffect(() => {
    if (active && mode === 'trace' && run?.execution_snapshot?.trace_supported === false)
      onFollowChange?.(follow)
  }, [active, mode, run?.execution_snapshot?.trace_supported, follow, onFollowChange])
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
            <ActionIcon name="refresh" />
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
      <div className="dag-node-context" tabIndex={-1}>
        <div className="dag-node-identity">
          {nodes.length > 1 ? (
            <ChoicePicker
              id={pickerId(node)}
              label={t('dag.selectNode')}
              searchLabel={t('dag.searchNodes')}
              popoverClassName="dag-node-picker"
              searchable
              value={node}
              options={nodes.map((n) => ({
                value: n.id,
                label: n.name,
                title: n.name,
                description: shortId(n.id),
                keywords: n.id,
                status: <Status state={n.state} />,
              }))}
              onChange={(next) => {
                onSelect(next)
                requestAnimationFrame(() => document.getElementById(pickerId(next))?.focus())
              }}
            />
          ) : (
            <strong>{name}</strong>
          )}
          {(execution.data?.attempts?.length ?? 0) > 1 && (
            <div className="dag-attempt-picker">
              <ChoicePicker
                label={t('retry.attempts')}
                popoverClassName="dag-attempt-menu"
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
        </div>
        {chosen && !run ? (
          <span className="dag-node-state-placeholder quiet text-muted">{t('正在加载…')}</span>
        ) : (
          <RunReason key={executionId ?? node} state={run?.state ?? state} reason={displayedReason}>
            <Status state={run?.state ?? state} />
          </RunReason>
        )}
        {!chosen && current?.retry_at && (
          <span className="quiet text-muted dag-retry-time">
            {t('retry.waiting')}{' '}
            <time dateTime={current.retry_at}>
              {new Date(current.retry_at).toLocaleTimeString(locale(), { hour12: false })}
            </time>
          </span>
        )}
        {startedAt && finishedAt && (
          <span className="dag-node-duration quiet text-muted mono">
            {Math.max(0, (Date.parse(finishedAt) - Date.parse(startedAt)) / 1000).toFixed(1)}s
          </span>
        )}
        <div className="dag-node-context-actions">
          {resultAction}
          {mode === 'trace' && executionId && (
            <TraceActions id={executionId} issues={trace.state.issues} />
          )}
          <div className="action-group" role="group" aria-label={t('actions.execution')}>
            {!chosen && user.role !== 'viewer' && execution.data?.retry && (
              <IconButton
                label={t(
                  execution.data.retry.action === 'accelerate'
                    ? 'retry.now'
                    : execution.data.retry.action === 'resume'
                      ? 'retry.resume'
                      : 'retry.node',
                )}
                disabledReason={
                  execution.data.retry.reason
                    ? apiErrorText(
                        execution.data.retry.reason,
                        t,
                        (state) => stateNames[state] ?? state,
                      )
                    : undefined
                }
                disabled={!execution.data.retry.action}
                loading={retry.isPending}
                onClick={() => retry.mutate(execution.data?.retry?.action === 'resume')}
              >
                <ActionIcon
                  name={
                    execution.data.retry.action === 'accelerate' ||
                    execution.data.retry.action === 'resume'
                      ? 'play'
                      : 'retry'
                  }
                />
              </IconButton>
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
                <ActionIcon name="reconcile" />
              </IconButton>
            )}
          </div>
        </div>
      </div>
      {!executionId ? (
        <p className="quiet text-muted dag-node-empty">
          {reason || stateNames[state] || t('dag.noExecution')}
        </p>
      ) : execution.error ? (
        <ErrorToast
          error={execution.error.message}
          actions={
            <Button onClick={() => execution.refetch()}>
              <RefreshCw size={14} />
              {t('重试')}
            </Button>
          }
        />
      ) : !run ? (
        <p className="quiet text-muted dag-node-empty">{t('正在加载…')}</p>
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
          <RunTrace
            key={run.id}
            run={run}
            trace={trace}
            viewport={viewport}
            active={active}
            onFollowChange={onFollowChange}
          />
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
              <RunResult result={run.result} raw={false} toast={toast} source={{ runId: run.id }} />
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
