import { ErrorToast } from '../Toast'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useCallback, useEffect, useState, useRef } from 'react'
import { MessageSquare, Terminal, Wrench, ChevronRight, RefreshCw } from 'lucide-react'
import { MarkdownContent } from './MarkdownContent'
import { api, terminal, type Run, type RunEvent, type TraceNode, stateNames } from '../api'
import { Button } from '../ui'
import { DetailBody } from '../Workbench'

type Entry = { firstSeq: number; lastSeq: number; node: TraceNode }
type TraceState = { nodes: Map<string, Entry>; notices: Set<string> }
const empty = (): TraceState => ({ nodes: new Map(), notices: new Set() })
function merge(state: TraceState, events: RunEvent[]): TraceState {
  const nodes = new Map(state.nodes)
  const notices = new Set(state.notices)
  for (const event of events) {
    if (event.kind === 'trace_status' && event.data.status) notices.add(event.data.status)
    if (event.kind !== 'trace') continue
    const node = event.data as unknown as TraceNode
    if (!node.id || !['message', 'tool', 'command'].includes(node.kind)) continue
    const old = nodes.get(node.id)
    const firstSeq = Math.min(old?.firstSeq ?? event.seq, event.seq)
    if (
      old &&
      (old.lastSeq >= event.seq || (old.node.status !== 'running' && node.status === 'running'))
    ) {
      if (old.firstSeq !== firstSeq) nodes.set(node.id, { ...old, firstSeq })
      continue
    }
    nodes.set(node.id, { firstSeq, lastSeq: event.seq, node })
  }
  return { nodes, notices }
}
export function useRunTrace(id: string, enabled = true) {
  const cursor = useRef(0)
  const previousId = useRef(id)
  const [state, setState] = useState<TraceState>(empty)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [attempt, setAttempt] = useState(0)
  const receive = useCallback((events: RunEvent[]) => setState((old) => merge(old, events)), [id])
  useEffect(() => {
    let cancelled = false
    if (previousId.current !== id) {
      previousId.current = id
      cursor.current = 0
      setState(empty())
    }
    if (!enabled) return
    setLoading(true)
    setError('')
    async function load() {
      try {
        for (;;) {
          const batch = await api<RunEvent[]>(
            `/runs/${id}/events?kind=trace&after=${cursor.current}`,
          )
          if (cancelled) return
          receive(batch)
          const next = Math.max(...batch.map((e) => e.id))
          if (batch.length && next <= cursor.current) throw new Error(tr('无法继续读取过程'))
          if (batch.length) cursor.current = next
          if (batch.length < 500) break
        }
      } catch (e) {
        if (!cancelled) setError((e as Error).message)
      } finally {
        if (!cancelled) setLoading(false)
      }
    }
    load()
    return () => {
      cancelled = true
    }
  }, [id, enabled, attempt, receive])
  return { state, loading, error, receive, retry: () => setAttempt((v) => v + 1) }
}
export function RunTrace({ run, trace }: { run: Run; trace: ReturnType<typeof useRunTrace> }) {
  useTranslation()
  const entries = [...trace.state.nodes.values()].sort((a, b) => a.firstSeq - b.firstSeq)
  const notices = trace.state.notices
  const support = run.execution_snapshot?.trace_supported
  let message = ''
  if (support === false) message = tr('此 Agent 未提供过程')
  else if (support === undefined && !entries.length)
    message = ['queued', 'starting'].includes(run.state) ? tr('等待执行') : tr('此运行未记录过程')
  else if (notices.has('expired') && !entries.length) message = tr('过程已过期')
  else if (!entries.length)
    message = trace.loading
      ? tr('正在加载…')
      : ['running', 'starting'].includes(run.state)
        ? tr('等待 Agent 事件…')
        : tr('未记录过程')
  return (
    <DetailBody>
      <div className="run-trace">
        {trace.error && (
          <ErrorToast>
            <span className="error">{trace.error}</span>
            <Button onClick={trace.retry}>
              <RefreshCw size={14} />
              {tr('重试')}
            </Button>
          </ErrorToast>
        )}
        {notices.has('expired') && entries.length > 0 && (
          <p className="quiet">{tr('部分过程已过期')}</p>
        )}
        {notices.has('truncated') && (
          <p className="quiet" role="status">
            {tr('过程已截断')}
          </p>
        )}
        {notices.has('incomplete') && (
          <p className="quiet" role="status">
            {tr('部分事件未记录')}
          </p>
        )}
        {message && <p className="quiet">{message}</p>}
        {entries.map(({ node }) => (
          <TraceRow
            key={node.id}
            node={node}
            ended={terminal(run.state) || run.state === 'unknown'}
            parent={node.parent_id ? trace.state.nodes.get(node.parent_id)?.node.title : undefined}
          />
        ))}
      </div>
    </DetailBody>
  )
}
function TraceRow({ node, parent, ended }: { node: TraceNode; parent?: string; ended: boolean }) {
  useTranslation()
  const [open, setOpen] = useState(node.kind === 'message')
  const Icon = node.kind === 'command' ? Terminal : node.kind === 'tool' ? Wrench : MessageSquare
  const elapsed =
    node.started_at && node.finished_at
      ? Date.parse(node.finished_at) - Date.parse(node.started_at)
      : NaN
  const duration =
    Number.isFinite(elapsed) && elapsed >= 0 ? `${(elapsed / 1000).toFixed(1)}s` : null
  const heading = (
    <>
      <Icon size={14} />
      <span className="trace-label">
        <span className="trace-title">{node.title}</span>
        {parent && (
          <span className="trace-parent" title={parent}>
            {parent}
          </span>
        )}
      </span>
      <span className={`trace-state ${node.status}`}>
        {ended && node.status === 'running' && !node.finished_at
          ? tr('未记录结束')
          : (stateNames[node.status] ?? node.status)}
      </span>
      <span className={duration ? 'trace-duration mono' : 'trace-duration-empty mono'}>
        {duration ?? '—'}
      </span>
    </>
  )
  return (
    <details className="trace-row trace-operation" open={open}>
      <summary
        onClick={(e) => {
          e.preventDefault()
          setOpen((v) => !v)
        }}
      >
        <ChevronRight size={14} />
        {heading}
      </summary>
      {node.kind === 'message' ? (
        <div className="markdown-result trace-message">
          <MarkdownContent text={valueText(node.output)} />
        </div>
      ) : (
        <div className="trace-payload">
          {node.input !== null && (
            <div>
              <h4>{tr('输入')}</h4>
              <pre className="code">{valueText(node.input)}</pre>
            </div>
          )}
          {node.output !== null && (
            <div>
              <h4>{tr('输出')}</h4>
              <pre className="code">{valueText(node.output)}</pre>
            </div>
          )}
        </div>
      )}
    </details>
  )
}
function valueText(value: unknown) {
  return value == null ? '' : typeof value === 'string' ? value : JSON.stringify(value, null, 2)
}
