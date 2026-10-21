import { useContext, useEffect, useLayoutEffect, useRef, useState, useCallback } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Link } from '@tanstack/react-router'
import {
  ArrowUpRight,
  Monitor,
  Copy,
  Check,
  GitBranch,
  RefreshCw,
  FileText,
  ExternalLink,
  Clock,
} from 'lucide-react'
import { t, locale } from '../i18n'
import { api, type MomentEntry, type Run, type User } from '../api'
import { IconButton, Button, TooltipTarget } from '../ui'
import { Status } from '../Status'
import { AgentLogo } from '../AgentLogo'
import { MarkdownContent } from '../runs/MarkdownContent'
import { CodeContent, isJsonCode } from '../runs/CodeContent'
import { dagOutputs, resultData } from '../runs/RunResult'
import { RunReason } from '../runs/RunReason'
import { ResumeRun } from '../sessions/ResumeRun'
import {
  duration,
  retriedNodes,
  reportIndex,
  type ReportHeading,
  type ReportLink,
} from './reportModel'
import { PreviewActivity } from '../files/PreviewActivity'
import { useReferenceCapacity } from './useReferenceCapacity'
import { VisitContext, manual } from './MomentsPage'

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
    if (!mounted || !card.current) return
    const observer = new ResizeObserver(([size]) => {
      const next = size.borderBoxSize[0]?.blockSize ?? size.contentRect.height
      if (next > 0) {
        setHeight(next)
        visit.heights.current.set(entry.id, next)
      }
    })
    observer.observe(card.current)
    return () => observer.disconnect()
  }, [mounted, entry.id, visit.heights])
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
            <MomentContent entry={entry} toast={toast} terminalActive={terminalActive} />
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
}: {
  entry: MomentEntry
  toast: (s: string) => void
  terminalActive: (open: boolean) => void
}) {
  useTranslation()
  const visit = useContext(VisitContext),
    client = useQueryClient()
  const me = useQuery<User>({ queryKey: ['me'], queryFn: () => api('/session'), enabled: false })
  const writable = me.data?.role !== 'viewer' && !me.data?.workspace?.is_demo
  const expanded = visit.expanded.has(entry.id)
  const report = useRef<HTMLDivElement>(null)
  const reading = useRef<HTMLDivElement>(null),
    context = useRef<HTMLElement>(null),
    referenceList = useRef<HTMLDivElement>(null),
    referenceToggle = useRef<HTMLButtonElement>(null)
  const [overflow, setOverflow] = useState(false)
  const [output, setOutput] = useState(visit.outputs.current.get(entry.id))
  const [headings, setHeadings] = useState<ReportHeading[]>([]),
    [links, setLinks] = useState<ReportLink[]>([])
  const [moreLinks, setMoreLinks] = useState(false),
    [moreHeadings, setMoreHeadings] = useState(false)
  const [copying, setCopying] = useState(false),
    [copied, setCopied] = useState(false)
  const referenceCapacity = useReferenceCapacity({
    reading,
    context,
    list: referenceList,
    toggle: referenceToggle,
    count: links.length,
  })
  const locate = useRef<number | null>(null)
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
  const viewKey = `${entry.id}:${nodeId ?? ''}`
  useLayoutEffect(() => {
    const root = report.current
    if (!root) return
    const index = reportIndex(root)
    setHeadings(index.headings)
    setLinks(index.links)
    const measure = () => setOverflow(root.scrollHeight > 138)
    measure()
    const observer = new ResizeObserver(measure)
    if (root.firstElementChild) observer.observe(root.firstElementChild)
    return () => observer.disconnect()
  }, [text, json, viewKey])
  useLayoutEffect(() => {
    if (!expanded || incomplete || locate.current === null || !report.current) return
    const target = report.current.querySelectorAll<HTMLElement>('h1,h2,h3,h4,h5,h6')[locate.current]
    if (target) {
      target.tabIndex = -1
      target.scrollIntoView({ block: 'nearest' })
      target.focus({ preventScroll: true })
    }
    locate.current = null
  }, [expanded, incomplete, text])
  function choose(id: string) {
    setOutput(id)
    visit.outputs.current.set(entry.id, id)
    setMoreLinks(false)
    setMoreHeadings(false)
    locate.current = null
  }
  function heading(index: number) {
    locate.current = index
    if (!expanded) visit.setExpanded(entry.id, true)
    else {
      const target = report.current?.querySelectorAll<HTMLElement>('h1,h2,h3,h4,h5,h6')[index]
      if (target) {
        target.tabIndex = -1
        target.scrollIntoView({ block: 'nearest' })
        target.focus({ preventScroll: true })
      }
      locate.current = null
    }
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
      await navigator.clipboard.writeText(resultData(result).source ?? '')
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch (e) {
      toast((e as Error).message)
    } finally {
      setCopying(false)
    }
  }
  const elapsed = duration(full.data?.started_at, full.data?.finished_at)
  const retries = retriedNodes(full.data)
  const hasInfo = outputs.length > 1 || headings.length > 1 || links.length > 0
  const resumable = isDag
    ? !!full.data?.nodes?.find((n) => n.id === nodeId)?.continuation_available
    : entry.continuation_available
  return (
    <div className="content-card moment-card">
      <header className="card-header">
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
              label={t('复制结果')}
              tooltip={copied ? t('已复制') : t('复制结果')}
              loading={copying}
              onClick={copy}
            >
              {copied ? <Check /> : <Copy />}
            </IconButton>
          )}
          <TooltipTarget label={t('查看运行')}>
            <Link
              className="button ghost icon-button"
              to="/runs"
              search={{ run: entry.id }}
              aria-label={t('查看运行')}
            >
              <ArrowUpRight size={16} />
            </Link>
          </TooltipTarget>
        </div>
      </header>
      <div className={`moment-content ${hasInfo ? 'moment-with-context' : ''}`}>
        <div className="moment-reading" ref={reading}>
          {retries > 0 && (
            <p className="moment-retries quiet">{t('moments.retried', { count: retries })}</p>
          )}
          {isDag && chosen && (
            <div className="moment-output-context">
              <span>{chosen.name}</span>
              <RunReason state={chosen.state} reason={chosen.reason}>
                <Status state={chosen.state} />
              </RunReason>
            </div>
          )}
          {truncated && <p className="quiet moment-notice">{t('结果已截断')}</p>}
          <div
            className={`moment-report ${expanded ? 'moment-report-expanded' : 'moment-report-preview'}`}
            ref={report}
          >
            {!data.empty ? (
              json ? (
                <CodeContent text={text} json={isJsonCode(value)} className="result-raw" />
              ) : (
                <article className="markdown-result">
                  <MarkdownContent
                    text={text}
                    source={{ runId: entry.id, ...(nodeId ? { nodeId } : {}) }}
                  />
                </article>
              )
            ) : incomplete ? (
              <p className="quiet">{t('moments.loadOutput')}</p>
            ) : (
              <p className="quiet">{t('未返回结果')}</p>
            )}
          </div>
          <div className="moment-reading-actions">
            {(overflow || expanded || incomplete) && (
              <Button
                className="moment-read"
                loading={expanded && full.isFetching}
                onClick={() => visit.setExpanded(entry.id, !expanded)}
              >
                {expanded ? t('收起') : t('阅读全文')}
              </Button>
            )}
            {full.error && (
              <Button onClick={() => full.refetch()}>
                <RefreshCw size={13} />
                {t('重试')}
              </Button>
            )}
          </div>
        </div>
        {hasInfo && (
          <aside ref={context} className="moment-context" aria-label={t('moments.related')}>
            {outputs.length > 1 && (
              <section className="moment-context-section">
                <h4>{t('moments.outputTitle')}</h4>
                <div className="moment-output-list">
                  {outputs.map((o) => (
                    <button
                      key={o.node_id}
                      className={nodeId === o.node_id ? 'selected' : ''}
                      aria-pressed={nodeId === o.node_id}
                      title={`${o.name} · ${o.node_id}`}
                      aria-label={`${o.name} · ${o.node_id}`}
                      onClick={() => choose(o.node_id)}
                    >
                      <span>
                        {o.name}
                        {outputs.some(
                          (other) => other.node_id !== o.node_id && other.name === o.name,
                        ) && <small className="moment-output-id">{o.node_id.slice(0, 8)}</small>}
                      </span>
                      <Status state={o.state} />
                    </button>
                  ))}
                </div>
              </section>
            )}
            {headings.length > 1 && (
              <section className="moment-context-section">
                <h4>
                  {t('moments.outline')}
                  {(incomplete || truncated) && (
                    <span className="quiet">{t('moments.excerpt')}</span>
                  )}
                </h4>
                <nav aria-label={t('moments.outline')}>
                  {(moreHeadings ? headings : headings.slice(0, 3)).map((h) => (
                    <button key={h.index} title={h.label} onClick={() => void heading(h.index)}>
                      {h.label}
                    </button>
                  ))}
                </nav>
                {headings.length > 3 && (
                  <button className="moment-more" onClick={() => setMoreHeadings((v) => !v)}>
                    {moreHeadings ? t('收起') : t('moments.more', { count: headings.length - 3 })}
                  </button>
                )}
              </section>
            )}
            {!!links.length && (
              <section className="moment-context-section">
                <h4>
                  {t('moments.links')}
                  {(incomplete || truncated) && (
                    <span className="quiet">{t('moments.excerpt')}</span>
                  )}
                </h4>
                <div className="moment-link-list" ref={referenceList}>
                  {links.map((link, index) =>
                    link.local ? (
                      <button
                        key={link.href}
                        hidden={!moreLinks && index >= referenceCapacity}
                        title={
                          link.image || !writable
                            ? `${t('filePreview.copyPath')}: ${link.href}`
                            : link.href
                        }
                        onClick={() =>
                          link.image
                            ? navigator.clipboard
                                .writeText(link.href)
                                .catch(() => toast(t('请选中文本后复制')))
                            : report.current
                                ?.querySelectorAll<HTMLElement>(
                                  'a[href],[data-result-file-href],[data-result-image-href]',
                                )
                                [link.index]?.click()
                        }
                      >
                        <FileText size={13} />
                        <span>{link.label}</span>
                        {(link.image || !writable) && <Copy size={12} />}
                      </button>
                    ) : (
                      <a
                        key={link.href}
                        hidden={!moreLinks && index >= referenceCapacity}
                        href={link.href}
                        target="_blank"
                        rel="noopener noreferrer"
                        title={link.href}
                      >
                        <ExternalLink size={13} />
                        <span>{link.label}</span>
                      </a>
                    ),
                  )}
                </div>
                <button
                  ref={referenceToggle}
                  className="moment-more"
                  hidden={!moreLinks && links.length <= referenceCapacity}
                  onClick={() => setMoreLinks((v) => !v)}
                >
                  {moreLinks
                    ? t('收起')
                    : t('moments.more', { count: Math.max(0, links.length - referenceCapacity) })}
                </button>
              </section>
            )}
          </aside>
        )}
      </div>
    </div>
  )
}
