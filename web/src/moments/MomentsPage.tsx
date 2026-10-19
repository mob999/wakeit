import { ErrorToast } from '../Toast'
import { t as tr, locale } from '../i18n'
import { useTranslation } from 'react-i18next'
import { PageBreadcrumbs, PageActions } from '../PageHeader'
import {
  createContext,
  useContext,
  useState,
  useRef,
  useLayoutEffect,
  useEffect,
  type ReactNode,
} from 'react'
import { useInfiniteQuery, useQuery, useQueryClient, useIsFetching } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { ArrowUpRight, Monitor, RefreshCw, Copy, Check, GitBranch } from 'lucide-react'
import { api, type MomentDays, type MomentEntries, type MomentEntry, type Run } from '../api'
import { IconButton, Button, TooltipTarget } from '../ui'
import { Status } from '../Status'
import { AgentLogo } from '../AgentLogo'
import { MarkdownContent } from '../runs/MarkdownContent'
import { RunResult, resultData } from '../runs/RunResult'
import { ResumeRun } from '../sessions/ResumeRun'
import './moments.css'

// This state lives only for the authenticated app visit, including trips to Run details.
const VisitContext = createContext<{
  expanded: Set<string>
  setExpanded: (id: string, value: boolean) => void
  scroll: { current: number }
}>(null!)
export function MomentsVisit({ children }: { children: ReactNode }) {
  useTranslation()
  const [expanded, setExpanded] = useState(new Set<string>())
  const scroll = useRef(0)
  return (
    <VisitContext.Provider
      value={{
        expanded,
        scroll,
        setExpanded: (id, value) =>
          setExpanded((previous) => {
            const next = new Set(previous)
            if (value) next.add(id)
            else next.delete(id)
            return next
          }),
      }}
    >
      {children}
    </VisitContext.Provider>
  )
}
const manual = {
  staleTime: Infinity,
  gcTime: Infinity,
  refetchOnWindowFocus: false,
  refetchOnReconnect: false,
  refetchOnMount: false,
  retry: false,
} as const
export function MomentsPage({ toast }: { toast: (message: string) => void }) {
  useTranslation()
  const client = useQueryClient()
  const visit = useContext(VisitContext)
  const viewport = useRef<HTMLDivElement>(null)
  const [refreshing, setRefreshing] = useState(false)
  const fetching = useIsFetching({ queryKey: ['moments'] })
  const anchor = useRef<{ id: string; top: number; position: number } | null>(null)
  useLayoutEffect(() => {
    if (refreshing || fetching || !anchor.current || !viewport.current) return
    const saved = anchor.current
    const node = viewport.current.querySelector<HTMLElement>(`[data-moment-id="${saved.id}"]`)
    if (node) viewport.current.scrollTop += node.getBoundingClientRect().top - saved.top
    else viewport.current.scrollTop = saved.position
    anchor.current = null
  }, [refreshing, fetching])
  const days = useInfiniteQuery({
    queryKey: ['moments', 'days'],
    queryFn: ({ pageParam }) =>
      api<MomentDays>(`/moments/days?limit=14${pageParam ? `&before=${pageParam}` : ''}`),
    initialPageParam: null as string | null,
    getNextPageParam: (last) => last.next_before,
    ...manual,
  })
  const first = days.data?.pages[0]
  useLayoutEffect(() => {
    if (viewport.current) viewport.current.scrollTop = visit.scroll.current
  }, [])
  async function refresh() {
    if (refreshing) return
    setRefreshing(true)
    const position = viewport.current?.scrollTop ?? 0
    const bounds = viewport.current?.getBoundingClientRect()
    const visible = Array.from(
      viewport.current?.querySelectorAll<HTMLElement>('[data-moment-id]') ?? [],
    ).find((node) => node.getBoundingClientRect().bottom > (bounds?.top ?? 0))
    anchor.current = {
      id: visible?.dataset.momentId ?? '',
      top: visible?.getBoundingClientRect().top ?? 0,
      position,
    }
    try {
      const result = await days.refetch()
      if (result.error) throw result.error
      await client.refetchQueries(
        { queryKey: ['moments', 'entries', result.data?.pages[0].timezone], type: 'all' },
        { throwOnError: true },
      )
    } catch (e) {
      toast((e as Error).message)
    } finally {
      setRefreshing(false)
    }
  }
  const yesterday = first ? new Date(`${first.today}T12:00:00Z`) : null
  yesterday?.setUTCDate(yesterday.getUTCDate() - 1)
  const yesterdayKey = yesterday?.toISOString().slice(0, 10)
  return (
    <div
      className="moments-page"
      ref={viewport}
      onScroll={(event) => {
        visit.scroll.current = event.currentTarget.scrollTop
      }}
    >
      <PageBreadcrumbs label={tr('Moments')} />
      <PageActions>
        <IconButton
          label={tr('刷新动态')}
          onClick={refresh}
          loading={refreshing}
          disabled={days.isFetching}
        >
          <RefreshCw />
        </IconButton>
      </PageActions>
      {days.isPending && <p className="quiet moments-state">{tr('正在加载…')}</p>}
      {days.error && <RequestError error={days.error} retry={() => days.refetch()} />}
      {first?.total_runs === 0 && <p className="quiet moments-state">{tr('暂无执行结果')}</p>}
      {first && (
        <div className="moments-timeline">
          {days.data?.pages
            .flatMap((page) => page.days)
            .map((day) => (
              <section
                className="moment-day"
                key={`${first.timezone}:${day.date}`}
                aria-label={day.date}
              >
                <div className="moment-date">
                  <div className="moment-date-label">
                    <h2>
                      {day.date === first.today
                        ? tr('今天')
                        : day.date === yesterdayKey
                          ? tr('昨天')
                          : day.date}
                    </h2>
                    <span>{tr('moments.runCount', { count: day.count })}</span>
                  </div>
                </div>
                <DayEntries date={day.date} timezone={first.timezone} toast={toast} />
              </section>
            ))}
          {days.hasNextPage && (
            <Button
              className="moments-load"
              loading={days.isFetchingNextPage}
              onClick={() => days.fetchNextPage()}
            >
              {tr('加载更早')}
            </Button>
          )}
        </div>
      )}
    </div>
  )
}
function RequestError({ error, retry }: { error: Error; retry: () => unknown }) {
  useTranslation()
  return (
    <ErrorToast>
      {error.message}
      <Button onClick={() => retry()}>
        <RefreshCw size={14} />
        {tr('重试')}
      </Button>
    </ErrorToast>
  )
}
function DayEntries({
  date,
  timezone,
  toast,
}: {
  date: string
  timezone: string
  toast: (s: string) => void
}) {
  useTranslation()
  const entries = useInfiniteQuery({
    queryKey: ['moments', 'entries', timezone, date],
    queryFn: ({ pageParam }) =>
      api<MomentEntries>(
        `/moments?date=${date}&timezone=${encodeURIComponent(timezone)}&limit=20${pageParam ? `&cursor=${encodeURIComponent(pageParam)}` : ''}`,
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (last) => last.next_cursor,
    ...manual,
  })
  return (
    <>
      {entries.isPending && <p className="quiet moments-state">{tr('正在加载…')}</p>}
      {entries.error && <RequestError error={entries.error} retry={() => entries.refetch()} />}
      {entries.data?.pages
        .flatMap((page) => page.items)
        .map((entry) => (
          <MomentCard key={entry.id} entry={entry} timezone={timezone} toast={toast} />
        ))}
      {entries.hasNextPage && (
        <Button
          className="moments-load"
          loading={entries.isFetchingNextPage}
          onClick={() => entries.fetchNextPage()}
        >
          {tr('加载更多')}
        </Button>
      )}
    </>
  )
}
function MomentCard({
  entry,
  timezone,
  toast,
}: {
  entry: MomentEntry
  timezone: string
  toast: (s: string) => void
}) {
  useTranslation()
  const visit = useContext(VisitContext)
  const client = useQueryClient()
  const expanded = visit.expanded.has(entry.id)
  const [overflow, setOverflow] = useState(false)
  const body = useRef<HTMLDivElement>(null)
  const [copying, setCopying] = useState(false)
  const [copied, setCopied] = useState(false)
  const full = useQuery({
    queryKey: ['run', entry.id],
    queryFn: () => api<Run>(`/runs/${entry.id}`),
    enabled: expanded && entry.has_more,
    ...manual,
  })
  const content = expanded && entry.has_more && full.data ? resultData(full.data.result) : null
  const text = content
    ? typeof content.text === 'string'
      ? content.text
      : content.source
    : entry.preview
  const format = content ? (typeof content.text === 'string' ? 'markdown' : 'json') : entry.format
  useLayoutEffect(() => {
    const node = body.current
    if (!node || expanded) return
    const measure = () => setOverflow(node.scrollHeight > 240)
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(node.firstElementChild ?? node)
    return () => observer.disconnect()
  }, [text, expanded])
  useEffect(() => {
    if (body.current && !expanded) body.current.scrollTop = 0
  }, [expanded])
  async function copy() {
    setCopying(true)
    try {
      const value =
        entry.has_more || entry.format === 'dag'
          ? (resultData(
              (
                await client.fetchQuery({
                  queryKey: ['run', entry.id],
                  queryFn: () => api<Run>(`/runs/${entry.id}`),
                  staleTime: Infinity,
                })
              ).result,
            ).source ?? '')
          : (entry.preview ?? '')
      await navigator.clipboard.writeText(value)
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch (e) {
      toast((e as Error).message)
    } finally {
      setCopying(false)
    }
  }
  const time = new Date(entry.finished_at).toLocaleTimeString(locale(), {
    timeZone: timezone,
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  })
  return (
    <article className="moment-entry" data-moment-id={entry.id}>
      <time className="moment-time" dateTime={entry.finished_at}>
        {time}
      </time>
      <span className="moment-node" aria-hidden="true" />
      <div className="content-card moment-card">
        <header className="card-header">
          <h3>
            <Link to="/runs" search={{ run: entry.id }}>
              {entry.name}
            </Link>
          </h3>
          <div className="moment-identity">
            {entry.format === 'dag' ? (
              <span>
                <GitBranch size={13} />
                DAG{' '}
                <span className="quiet">
                  {tr('dag.nodeCount', { count: entry.node_count ?? 0 })}
                </span>
              </span>
            ) : (
              <>
                <span>
                  <Monitor size={13} />
                  {entry.runner}
                </span>
                <span>
                  <AgentLogo name={entry.agent} logo={entry.logo} />
                  {entry.agent}
                </span>
              </>
            )}
          </div>
          <div className="moment-actions">
            <Status state={entry.state} />
            <ResumeRun id={entry.id} available={entry.continuation_available} toast={toast} />
            {(entry.preview || !!entry.outputs?.length) && (
              <IconButton
                label={tr('复制结果')}
                tooltip={copied ? tr('已复制') : tr('复制结果')}
                disabled={copying}
                loading={copying}
                onClick={copy}
              >
                {copied ? <Check /> : <Copy />}
              </IconButton>
            )}
            <TooltipTarget label={tr('查看运行')}>
              <Link
                className="button ghost icon-button"
                to="/runs"
                search={{ run: entry.id }}
                aria-label={tr('查看运行')}
              >
                <ArrowUpRight size={16} />
              </Link>
            </TooltipTarget>
          </div>
        </header>
        <div className="card-body">
          {entry.reason && entry.state !== 'succeeded' && (
            <p className="moment-reason">{entry.reason}</p>
          )}
          {entry.truncated && <p className="quiet">{tr('结果已截断')}</p>}
          {text || entry.outputs?.length ? (
            <div
              ref={body}
              className={`moment-report ${expanded ? 'moment-report-expanded' : 'moment-report-preview'}`}
            >
              {entry.format === 'dag' ? (
                content && full.data ? (
                  <RunResult result={full.data.result} raw={false} />
                ) : (
                  <div className="moment-dag-reports">
                    {entry.outputs?.map((output) => (
                      <section key={output.node_id}>
                        <header>
                          <strong>{output.name}</strong>
                          <Status state={output.state} />
                        </header>
                        {output.reason && output.state !== 'succeeded' && (
                          <p className="quiet">{output.reason}</p>
                        )}
                        {output.preview ? (
                          output.format === 'markdown' ? (
                            <article className="markdown-result">
                              <MarkdownContent text={output.preview} />
                            </article>
                          ) : (
                            <pre className="code result-raw">{output.preview}</pre>
                          )
                        ) : (
                          <p className="quiet">{tr('未返回结果')}</p>
                        )}
                      </section>
                    ))}
                  </div>
                )
              ) : format === 'markdown' ? (
                <article className="markdown-result">
                  <MarkdownContent text={text ?? ''} />
                </article>
              ) : (
                <pre className="code result-raw">{text}</pre>
              )}
            </div>
          ) : (
            !entry.reason && <p className="quiet">{tr('未返回结果')}</p>
          )}
          {full.error && expanded && (
            <RequestError error={full.error} retry={() => full.refetch()} />
          )}
          {(entry.has_more || overflow || expanded) && (
            <Button
              className="moment-read"
              loading={expanded && full.isFetching}
              onClick={() => visit.setExpanded(entry.id, !expanded)}
            >
              {expanded ? tr('收起') : tr('阅读全文')}
            </Button>
          )}
        </div>
      </div>
    </article>
  )
}
