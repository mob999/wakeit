import { ActionIcon } from '../actions/ActionIcon'
import { ErrorToast } from '../Toast'
import { t as tr } from '../i18n'
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
import { RefreshCw } from '../icons'
import { api, type MomentDays, type MomentEntries } from '../api'
import { IconButton, Button } from '../ui'
import './moments.css'
import { MomentCard } from './MomentCard'
import { ReportVisit } from '../reports/ReportVisit'

// This state lives only for the authenticated app visit, including trips to Run details.
export const VisitContext = createContext<{
  expanded: Set<string>
  setExpanded: (id: string, value: boolean) => void
  scroll: { current: number }
  outputs: { current: Map<string, string> }
  heights: { current: Map<string, number> }
}>(null!)
export function MomentsVisit({ children }: { children: ReactNode }) {
  useTranslation()
  const [expanded, setExpanded] = useState(new Set<string>())
  const scroll = useRef(0)
  const heights = useRef(new Map<string, number>())
  const outputs = useRef(new Map<string, string>())
  return (
    <ReportVisit>
      <VisitContext.Provider
        value={{
          expanded,
          scroll,
          outputs,
          heights,
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
    </ReportVisit>
  )
}
export const manual = {
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
    viewport.current.style.overflowAnchor = ''
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
    if (viewport.current) viewport.current.style.overflowAnchor = 'none'
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
      await client.invalidateQueries({ queryKey: ['moments', 'report'], refetchType: 'active' })
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
      onWheel={() => {
        anchor.current = null
        if (viewport.current) viewport.current.style.overflowAnchor = ''
      }}
      onTouchStart={() => {
        anchor.current = null
        if (viewport.current) viewport.current.style.overflowAnchor = ''
      }}
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
          <ActionIcon name="refresh" />
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
