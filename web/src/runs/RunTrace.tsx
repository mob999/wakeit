import { ErrorToast } from '../Toast'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useCallback, useEffect, useLayoutEffect, useState, useRef, type RefObject } from 'react'
import { MessageSquare, Terminal, Wrench, ChevronRight, RefreshCw, Check, Minus } from '../icons'
import { MarkdownContent } from './MarkdownContent'
import { api, terminal, type Run, type RunEvent, type TraceNode, stateNames } from '../api'
import { Button } from '../ui'
import { useRunReading } from './RunReading'
import {
  valueText,
  visibleSummary,
  inputSummary,
  previewLines,
  isExpanded,
  followingTail,
} from './readingModel'
import { CopyValue } from './CopyValue'
import { CodeContent, isJsonCode } from './CodeContent'
import { useToast } from '../Toast'
import { recordingReason, type RecordingReason } from './recordingInfo'

type Entry = { firstSeq: number; lastSeq: number; node: TraceNode }
type TraceState = { nodes: Map<string, Entry>; notices: Set<string>; issues: Set<RecordingReason> }
const empty = (): TraceState => ({ nodes: new Map(), notices: new Set(), issues: new Set() })
function merge(state: TraceState, events: RunEvent[]): TraceState {
  const nodes = new Map(state.nodes)
  const notices = new Set(state.notices)
  const issues = new Set(state.issues)
  for (const event of events) {
    if (event.kind === 'trace_status' && event.data.status) {
      notices.add(event.data.status)
      const reason = recordingReason(event.data)
      if (reason) issues.add(reason)
    }
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
  return { nodes, notices, issues }
}
export function useRunTrace(id: string, enabled = true) {
  const cursor = useRef(0)
  const previousId = useRef(id)
  const receivingId = useRef(id)
  receivingId.current = id
  const [state, setState] = useState<TraceState>(empty)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [attempt, setAttempt] = useState(0)
  const receive = useCallback(
    (events: RunEvent[]) => {
      if (receivingId.current === id) setState((old) => merge(old, events))
    },
    [id],
  )
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
  return {
    state: previousId.current === id ? state : empty(),
    loading,
    error,
    receive,
    retry: () => setAttempt((v) => v + 1),
  }
}
export function RunTrace({
  run,
  trace,
  viewport,
  active = true,
  onFollowChange,
}: {
  run: Run
  trace: ReturnType<typeof useRunTrace>
  viewport?: RefObject<HTMLDivElement | null>
  active?: boolean
  onFollowChange?: (follow: boolean) => void
}) {
  useTranslation()
  const [reading, update] = useRunReading(`execution:${run.id}`)
  const current = useRef(reading)
  current.current = reading
  const entries = [...trace.state.nodes.values()].sort((a, b) => a.firstSeq - b.firstSeq)
  const revision = entries.map((e) => `${e.node.id}:${e.lastSeq}`).join('|')
  const noticeRevision = [...trace.state.notices].sort().join('|')
  const latest = [...entries]
    .reverse()
    .find((e) => e.node.kind === 'message' && valueText(e.node.output).trim())?.node.id
  const root = useRef<HTMLDivElement>(null),
    tail = useRef<HTMLDivElement>(null)
  const anchor = useRef(reading.anchor),
    previous = useRef<Map<string, number> | null>(null)
  const jump = useRef(reading.jump),
    restoring = useRef(false)
  const getHost = () =>
    viewport?.current ?? root.current?.closest<HTMLDivElement>('.run-page') ?? null
  function capture() {
    const host = getHost()
    if (!host || !root.current) return
    const top = host.getBoundingClientRect().top + 48
    const row = [...root.current.querySelectorAll<HTMLElement>('[data-trace-id]')].find(
      (el) => el.getBoundingClientRect().bottom > top,
    )
    if (row)
      anchor.current = {
        id: row.dataset.traceId!,
        offset: row.getBoundingClientRect().top - host.getBoundingClientRect().top,
      }
  }
  useEffect(() => {
    if (active) onFollowChange?.(reading.follow)
  }, [reading.follow, active, onFollowChange])
  useEffect(() => {
    if (!active) return
    const host = getHost()
    if (!host) return
    let frame = 0
    const onScroll = () => {
      if (restoring.current) return
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => {
        capture()
        if (!tail.current) return
        const follow = followingTail(
          tail.current.getBoundingClientRect().bottom - host.getBoundingClientRect().bottom,
        )
        if (follow !== current.current.follow || (follow && current.current.unseen.length))
          update({ follow, unseen: follow ? [] : current.current.unseen })
      })
    }
    host.addEventListener('scroll', onScroll, { passive: true })
    return () => {
      host.removeEventListener('scroll', onScroll)
      cancelAnimationFrame(frame)
      update({ anchor: anchor.current })
    }
  }, [run.id, active, update])
  useLayoutEffect(() => {
    if (!active) return
    const host = getHost()
    if (!host) return
    const versions = new Map(entries.map((e) => [e.node.id, e.lastSeq]))
    if (!trace.loading) {
      if (previous.current && !reading.follow) {
        const unseen = new Set(reading.unseen)
        for (const [id, seq] of versions) if (previous.current.get(id) !== seq) unseen.add(id)
        if (unseen.size !== reading.unseen.length) update({ unseen: [...unseen] })
      }
      previous.current = versions
    }
    const jumped = jump.current !== reading.jump
    jump.current = reading.jump
    if (reading.follow && latest && reading.autoId !== latest) update({ autoId: latest })
    if (jumped && latest)
      update((old) => ({ ...old, expanded: { ...old.expanded, [latest]: true } }))
    restoring.current = true
    if (reading.follow && tail.current)
      host.scrollTop +=
        tail.current.getBoundingClientRect().bottom - host.getBoundingClientRect().bottom
    else if (anchor.current) {
      const row = [...root.current!.querySelectorAll<HTMLElement>('[data-trace-id]')].find(
        (el) => el.dataset.traceId === anchor.current!.id,
      )
      if (row)
        host.scrollTop +=
          row.getBoundingClientRect().top - host.getBoundingClientRect().top - anchor.current.offset
    }
    capture()
    const frame = requestAnimationFrame(() => {
      restoring.current = false
    })
    return () => {
      cancelAnimationFrame(frame)
      restoring.current = false
    }
  }, [
    run.id,
    active,
    revision,
    noticeRevision,
    trace.loading,
    reading.follow,
    reading.jump,
    reading.expanded,
    reading.full,
    reading.payload,
    reading.autoId,
  ])
  const notices = trace.state.notices,
    support = run.execution_snapshot?.trace_supported
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
    <div className="run-trace" ref={root} data-execution={run.id} data-follow={reading.follow}>
      {trace.error && (
        <ErrorToast>
          <span>{trace.error}</span>
          <Button onClick={trace.retry}>
            <RefreshCw size={14} />
            {tr('重试')}
          </Button>
        </ErrorToast>
      )}
      {message && <p className="quiet trace-retention">{message}</p>}
      {entries.map(({ node }) => (
        <TraceRow
          key={node.id}
          node={node}
          ended={terminal(run.state) || run.state === 'unknown'}
          parent={
            node.parent_id
              ? visibleSummary(trace.state.nodes.get(node.parent_id)?.node.output) ||
                trace.state.nodes.get(node.parent_id)?.node.title
              : undefined
          }
          open={isExpanded(reading, node.id, latest)}
          toggle={() =>
            update((old) => ({
              ...old,
              expanded: { ...old.expanded, [node.id]: !isExpanded(old, node.id, latest) },
            }))
          }
          execution={run.id}
        />
      ))}
      <div ref={tail} className="trace-tail" />
    </div>
  )
}
function TraceRow({
  node,
  parent,
  ended,
  open,
  toggle,
  execution,
}: {
  node: TraceNode
  parent?: string
  ended: boolean
  open: boolean
  toggle: () => void
  execution: string
}) {
  const Icon = node.kind === 'command' ? Terminal : node.kind === 'tool' ? Wrench : MessageSquare
  const elapsed =
    node.started_at && node.finished_at
      ? Date.parse(node.finished_at) - Date.parse(node.started_at)
      : NaN
  const duration =
    Number.isFinite(elapsed) && elapsed >= 0 ? `${(elapsed / 1000).toFixed(1)}s` : null
  const status =
    ended && node.status === 'running' && !node.finished_at
      ? tr('未记录结束')
      : (stateNames[node.status] ?? node.status)
  const hasContent = !!(
    valueText(node.output).trim() ||
    (node.kind !== 'message' && valueText(node.input).trim())
  )
  const summary = node.kind === 'message' ? visibleSummary(node.output) : inputSummary(node.input)
  const heading = (
    <>
      <ChevronRight size={14} aria-hidden="true" />
      <Icon size={14} aria-hidden="true" />
      <span className="trace-label" title={summary || node.title}>
        {node.kind !== 'message' && <span className="trace-title">{node.title}</span>}
        <span className="trace-summary">{summary || node.title}</span>
        {parent && (
          <span className="trace-parent" title={parent}>
            {parent}
          </span>
        )}
      </span>
      <span className={`trace-state ${node.status}`} title={status} aria-label={status}>
        {node.status === 'succeeded' ? (
          <>
            <Check size={12} aria-hidden="true" />
            <span className="sr-only">{status}</span>
          </>
        ) : node.status === 'cancelled' ? (
          <>
            <Minus size={12} aria-hidden="true" />
            <span className="sr-only">{status}</span>
          </>
        ) : (
          status
        )}
      </span>
      <span className={duration ? 'trace-duration mono' : 'trace-duration-empty mono'}>
        {duration ?? '—'}
      </span>
    </>
  )
  if (!hasContent)
    return (
      <div
        className="trace-row trace-empty"
        data-trace-id={node.id}
        data-parent-id={node.parent_id ?? undefined}
      >
        <div className="trace-heading">{heading}</div>
      </div>
    )
  return (
    <details
      className="trace-row trace-operation"
      data-trace-id={node.id}
      data-parent-id={node.parent_id ?? undefined}
      open={open}
    >
      <summary
        onClick={(e) => {
          e.preventDefault()
          toggle()
        }}
      >
        {heading}
      </summary>
      {open &&
        (node.kind === 'message' ? (
          <div className="markdown-result trace-message">
            <MarkdownContent text={valueText(node.output)} />
          </div>
        ) : (
          <div className="trace-payload">
            {node.input != null && (
              <TracePayload
                keyName={`${node.id}:input`}
                execution={execution}
                label={tr('输入')}
                value={node.input}
              />
            )}
            {node.output != null && (
              <TracePayload
                keyName={`${node.id}:output`}
                execution={execution}
                label={tr('输出')}
                value={node.output}
              />
            )}
          </div>
        ))}
    </details>
  )
}
function TracePayload({
  keyName,
  execution,
  label,
  value,
}: {
  keyName: string
  execution: string
  label: string
  value: unknown
}) {
  const [reading, update] = useRunReading(`execution:${execution}`),
    toast = useToast()
  const open = reading.payload[keyName] ?? true,
    full = reading.full[keyName] ?? false,
    text = valueText(value),
    preview = previewLines(text, full)
  return (
    <section className="trace-payload-section">
      <div className="trace-payload-heading">
        <button
          type="button"
          aria-expanded={open}
          onClick={() =>
            update((old) => ({ ...old, payload: { ...old.payload, [keyName]: !open } }))
          }
        >
          <ChevronRight size={12} className={open ? 'expanded' : ''} />
          {label}
        </button>
        <span>
          {preview.truncated && open && (
            <button
              type="button"
              className="trace-payload-full"
              onClick={() => update((old) => ({ ...old, full: { ...old.full, [keyName]: !full } }))}
            >
              {tr(full ? 'reading.less' : 'reading.full')}
            </button>
          )}
          <CopyValue value={text} label={tr('复制{{v0}}', { v0: label })} toast={toast} />
        </span>
      </div>
      {open && <CodeContent text={preview.text} json={isJsonCode(value)} />}
    </section>
  )
}
