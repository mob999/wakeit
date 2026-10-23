import { ActionMenu, ActionMenuItem } from '../actions/ActionMenu'
import { useCopy } from '../actions/useCopy'
import { ActionIcon } from '../actions/ActionIcon'
import { useContext, useEffect, useLayoutEffect, useRef, useState, useCallback } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate } from '@tanstack/react-router'
import { Monitor, GitBranch, Clock } from '../icons'
import { t, locale } from '../i18n'
import { api, type MomentEntry, type Run } from '../api'
import { IconButton, TooltipTarget } from '../ui'
import { Status } from '../Status'
import { AgentLogo } from '../AgentLogo'
import { MarkdownContent } from '../runs/MarkdownContent'
import { CodeContent, isJsonCode } from '../runs/CodeContent'
import { dagOutputs, resultData } from '../runs/RunResult'
import { RunReason } from '../runs/RunReason'
import { ResumeRun } from '../sessions/ResumeRun'
import { duration, retriedNodes } from './reportModel'
import { PreviewActivity } from '../files/PreviewActivity'
import { ReportReader } from '../reports/ReportReader'
import { VisitContext, manual } from './MomentsPage'
import { momentCollapseAnchor } from './collapsePosition'

export function MomentCard({
  entry,
  timezone,
  toast,
}: {
  entry: MomentEntry
  timezone: string
  toast: (s: string) => void
}) {
  useTranslation()
  const visit = useContext(VisitContext),
    shell = useRef<HTMLElement>(null),
    card = useRef<HTMLDivElement>(null)
  const [near, setNear] = useState(false),
    [pinned, setPinned] = useState(false)
  const [height, setHeight] = useState(visit.heights.current.get(entry.id) ?? 260)
  const pins = useRef(new Set<string>())
  const originalOffset = useRef(0)
  const pendingCollapse = useRef<{
    viewport: HTMLElement
    node: HTMLElement
    top: number
    overflowAnchor: string
  } | null>(null)
  const expanded = visit.expanded.has(entry.id)
  function changeExpanded(value: boolean) {
    const element = shell.current
    const viewport = element?.closest<HTMLElement>('.moments-page')
    if (element && viewport) {
      const top = viewport.getBoundingClientRect().top
      if (value && !expanded) originalOffset.current = element.getBoundingClientRect().top - top
      if (!value && expanded) {
        const entries = Array.from(viewport.querySelectorAll<HTMLElement>('[data-moment-id]'))
        const anchor = momentCollapseAnchor(
          entry.id,
          entries.map((node) => {
            const rect = node.getBoundingClientRect()
            return { id: node.dataset.momentId!, top: rect.top, bottom: rect.bottom }
          }),
          top,
          originalOffset.current,
          card.current?.querySelector('.card-header')?.getBoundingClientRect().height ?? 44,
        )
        pendingCollapse.current = {
          viewport,
          node: entries.find((node) => node.dataset.momentId === anchor.id) ?? element,
          top: anchor.top,
          overflowAnchor: viewport.style.overflowAnchor,
        }
        viewport.style.overflowAnchor = 'none'
      }
    }
    visit.setExpanded(entry.id, value)
  }
  useLayoutEffect(() => {
    const anchor = pendingCollapse.current
    if (expanded || !anchor) return
    pendingCollapse.current = null
    let frame = 0
    const restore = () => {
      anchor.viewport.scrollTop += anchor.node.getBoundingClientRect().top - anchor.top
    }
    const finish = () => {
      cancelAnimationFrame(frame)
      observer.disconnect()
      for (const name of ['wheel', 'touchstart', 'pointerdown', 'keydown'])
        anchor.viewport.removeEventListener(name, finish)
      anchor.viewport.style.overflowAnchor = anchor.overflowAnchor
    }
    const settle = () => {
      restore()
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => {
        restore()
        frame = requestAnimationFrame(finish)
      })
    }
    const observer = new ResizeObserver(settle)
    if (card.current) observer.observe(card.current)
    observer.observe(anchor.node)
    for (const name of ['wheel', 'touchstart', 'pointerdown', 'keydown'])
      anchor.viewport.addEventListener(name, finish, { passive: true })
    settle()
    return finish
  }, [expanded])
  const pin = useCallback((id: string, open: boolean) => {
    if (open) pins.current.add(id)
    else pins.current.delete(id)
    setPinned(pins.current.size > 0)
  }, [])
  const terminalActive = useCallback((open: boolean) => pin('terminal', open), [pin])
  const mounted = near || pinned || visit.expanded.has(entry.id)
  useEffect(() => {
    if (!shell.current) return
    const observer = new IntersectionObserver(([entry]) => setNear(entry.isIntersecting), {
      root: shell.current.closest('.moments-page'),
      rootMargin: '600px 0px',
    })
    observer.observe(shell.current)
    return () => observer.disconnect()
  }, [])
  useLayoutEffect(() => {
    if (!mounted || expanded || !card.current) return
    const observer = new ResizeObserver(([size]) => {
      const next = size.borderBoxSize[0]?.blockSize ?? size.contentRect.height
      if (next > 0) {
        setHeight(next)
        visit.heights.current.set(entry.id, next)
      }
    })
    observer.observe(card.current)
    return () => observer.disconnect()
  }, [mounted, expanded, entry.id, visit.heights])
  return (
    <article className="moment-entry" ref={shell} data-moment-id={entry.id}>
      <time className="moment-time" dateTime={entry.finished_at}>
        {new Date(entry.finished_at).toLocaleTimeString(locale(), {
          timeZone: timezone,
          hour: '2-digit',
          minute: '2-digit',
          hour12: false,
        })}
      </time>
      <span className="moment-node" aria-hidden="true" />
      <div ref={card} className="moment-card-host">
        <PreviewActivity.Provider value={pin}>
          {mounted ? (
            <MomentContent
              entry={entry}
              toast={toast}
              terminalActive={terminalActive}
              onExpandedChange={changeExpanded}
            />
          ) : (
            <div
              className="content-card moment-card moment-placeholder"
              style={{ height }}
              aria-label={entry.name}
            />
          )}
        </PreviewActivity.Provider>
      </div>
    </article>
  )
}
function MomentContent({
  entry,
  toast,
  terminalActive,
  onExpandedChange,
}: {
  entry: MomentEntry
  toast: (s: string) => void
  terminalActive: (open: boolean) => void
  onExpandedChange: (value: boolean) => void
}) {
  useTranslation()
  const visit = useContext(VisitContext),
    client = useQueryClient()
  const expanded = visit.expanded.has(entry.id)
  const [output, setOutput] = useState(visit.outputs.current.get(entry.id))
  const [overflow, setOverflow] = useState(false)
  const [copying, setCopying] = useState(false)
  const { copied, copy: copyOutput } = useCopy(toast, t('actions.copyOutput'))
  const navigate = useNavigate()
  const queryKey = ['moments', 'report', entry.id]
  const full = useQuery({
    queryKey,
    queryFn: ({ signal }) => api<Run>(`/runs/${entry.id}`, 'GET', undefined, undefined, signal),
    ...manual,
    refetchOnMount: true,
    gcTime: 60_000,
  })
  useEffect(() => {
    if (full.error) toast(full.error.message)
  }, [full.error, toast])
  const actualOutputs = dagOutputs(full.data?.result)
  const outputs = actualOutputs ?? entry.outputs ?? []
  const chosen = outputs.find((o) => o.node_id === output) ?? outputs[0]
  const nodeId = chosen?.node_id
  const execution = full.data?.nodes?.find((node) => node.id === nodeId)?.execution_id
  const fileSource = execution
    ? { runId: execution }
    : { runId: entry.id, ...(nodeId ? { nodeId } : {}) }
  useEffect(() => {
    if (nodeId) visit.outputs.current.set(entry.id, nodeId)
  }, [entry.id, nodeId, visit.outputs])
  const actual = actualOutputs?.find((o) => o.node_id === nodeId)
  const preview = entry.outputs?.find((o) => o.node_id === nodeId)
  const isDag = entry.format === 'dag'
  const value = isDag
    ? actual
      ? actual.result
      : preview?.preview
    : full.data
      ? full.data.result
      : entry.preview
  const data = resultData(value)
  const text = typeof data.text === 'string' ? data.text : (data.source ?? '')
  const json = isDag
    ? actual
      ? typeof data.text !== 'string'
      : preview?.format === 'json'
    : full.data
      ? typeof data.text !== 'string'
      : entry.format === 'json'
  const incomplete = !full.data && (isDag ? preview?.has_more : entry.has_more)
  const truncated = full.data
    ? data.truncated
    : data.truncated || (isDag ? preview?.truncated : entry.truncated)
  function choose(id: string) {
    setOutput(id)
    visit.outputs.current.set(entry.id, id)
  }
  async function copy() {
    setCopying(true)
    try {
      const current =
        full.data ??
        (await client.fetchQuery<Run>({
          queryKey,
          queryFn: () => api<Run>(`/runs/${entry.id}`),
          staleTime: Infinity,
        }))
      if (!current) throw new Error(t('未返回结果'))
      const result = isDag
        ? dagOutputs(current.result)?.find((o) => o.node_id === nodeId)?.result
        : current.result
      await copyOutput(resultData(result).source ?? '')
    } catch (e) {
      toast((e as Error).message)
    } finally {
      setCopying(false)
    }
  }
  const elapsed = duration(full.data?.started_at, full.data?.finished_at)
  const retries = retriedNodes(full.data)
  const resumable = isDag
    ? !!full.data?.nodes?.find((n) => n.id === nodeId)?.continuation_available
    : entry.continuation_available
  return (
    <div className="content-card moment-card">
      <header className="card-header">
        {(overflow || expanded || incomplete) && (
          <IconButton
            className="moment-read"
            label={expanded ? t('收起') : t('阅读全文')}
            aria-expanded={expanded}
            loading={expanded && full.isFetching}
            onClick={() => onExpandedChange(!expanded)}
          >
            {expanded ? <ActionIcon name="shrink" /> : <ActionIcon name="expand" />}
          </IconButton>
        )}
        <h3>
          <Link to="/runs" search={{ run: entry.id }}>
            {entry.name}
          </Link>
        </h3>
        <div className="moment-identity">
          {isDag ? (
            <span>
              <GitBranch size={12} />
              DAG · {t('dag.nodeCount', { count: entry.node_count ?? 0 })} ·{' '}
              {t('moments.outputs', { count: outputs.length })}
            </span>
          ) : (
            <>
              <span title={entry.runner}>
                <Monitor size={12} />
                <span className="moment-device">{entry.runner}</span>
              </span>
              <span>
                <AgentLogo name={entry.agent} logo={entry.logo} />
                <span className="moment-agent-name">{entry.agent}</span>
              </span>
            </>
          )}
        </div>

        <div className="moment-actions">
          {elapsed && (
            <TooltipTarget label={t('moments.elapsed')}>
              <span className="moment-elapsed">
                <Clock size={12} />
                {elapsed}
              </span>
            </TooltipTarget>
          )}
          <RunReason state={entry.state} reason={entry.reason}>
            <Status state={entry.state} />
          </RunReason>
          {full.error && (
            <IconButton
              label={t('actions.reloadResult')}
              loading={full.isFetching}
              onClick={() => full.refetch()}
            >
              <ActionIcon name="refresh" />
            </IconButton>
          )}
          <ResumeRun
            id={entry.id}
            available={resumable}
            initialNode={nodeId}
            strictNode={isDag}
            onActiveChange={terminalActive}
            toast={toast}
          />
          {(!data.empty || isDag) && (
            <IconButton
              label={t('actions.copyOutput')}
              tooltip={copied ? t('已复制') : t('actions.copyOutput')}
              loading={copying}
              onClick={copy}
            >
              {copied ? <ActionIcon name="check" /> : <ActionIcon name="copy" />}
            </IconButton>
          )}

          <ActionMenu label={t('actions.reportMenu')}>
            <ActionMenuItem
              icon="open"
              onSelect={() => navigate({ to: '/runs', search: { run: entry.id } })}
            >
              {t('查看运行')}
            </ActionMenuItem>
          </ActionMenu>
        </div>
      </header>
      <ReportReader
        identity={`${entry.id}:${nodeId ?? ''}:${execution ?? entry.id}`}
        indexKey={text}
        markdown={!json && !data.empty}
        source={fileSource}
        cwd={full.data?.nodes?.find((node) => node.id === nodeId)?.cwd ?? full.data?.config.cwd}
        outputs={isDag ? outputs : []}
        selectedOutput={nodeId}
        onSelectOutput={choose}
        preview={!expanded}
        incomplete={!!incomplete}
        onExpand={() => onExpandedChange(true)}
        notice={
          <>
            {retries > 0 && (
              <p className="moment-retries quiet">{t('moments.retried', { count: retries })}</p>
            )}
            {truncated && <p className="quiet moment-notice">{t('结果已截断')}</p>}
          </>
        }
        onOverflowChange={setOverflow}
      >
        {!data.empty ? (
          json ? (
            <CodeContent text={text} json={isJsonCode(value)} className="result-raw" />
          ) : (
            <article className="markdown-result">
              <MarkdownContent text={text} source={fileSource} />
            </article>
          )
        ) : incomplete ? (
          <p className="quiet">{t('moments.loadOutput')}</p>
        ) : (
          <p className="quiet">{t('未返回结果')}</p>
        )}
      </ReportReader>
    </div>
  )
}
