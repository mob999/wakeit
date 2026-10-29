import {
  createContext,
  useContext,
  useState,
  useRef,
  useLayoutEffect,
  useEffect,
  type ReactNode,
  type CSSProperties,
} from 'react'
import { useInfiniteQuery, useQueryClient, useIsFetching } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { ActionIcon } from '../actions/ActionIcon'
import { ErrorToast } from '../components/Toast'
import { PageBreadcrumbs, PageActions } from '../components/PageHeader'
import { ChoicePicker } from '../components/ChoicePicker'
import { PaneResizeHandle } from '../components/PaneResizeHandle'
import { api } from '../api/client'
import type { MomentDays, MomentEntries, MomentEntry } from '../api/types'
import { IconButton, Button } from '../components/ui'
import { MomentRow } from './MomentRow'
import { MomentReader } from './MomentReader'
import { MomentsOverview } from './MomentsOverview'
import { ReportVisit } from '../reports/ReportVisit'
import './moments.css'

type Anchor = { id: string; offset: number; position: number }
export const VisitContext = createContext<{
  selected: MomentEntry | null
  setSelected: (entry: MomentEntry | null) => void
  scroll: { current: number }
  origin: { current: Anchor | null }
  outputs: { current: Map<string, string> }
  ratio: number
  setRatio: (ratio: number) => void
  view: string
  setView: (view: string) => void
}>(null!)
export function MomentsVisit({ children }: { children: ReactNode }) {
  const [selected, setSelected] = useState<MomentEntry | null>(null)
  const [ratio, setRatio] = useState(0.65)
  const [view, setView] = useState('feed')
  const scroll = useRef(0),
    origin = useRef<Anchor | null>(null)
  const outputs = useRef(new Map<string, string>())
  return (
    <ReportVisit>
      <VisitContext.Provider
        value={{ selected, setSelected, scroll, origin, outputs, ratio, setRatio, view, setView }}
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
  const { t } = useTranslation()
  const client = useQueryClient(),
    visit = useContext(VisitContext)
  const viewport = useRef<HTMLDivElement>(null),
    layout = useRef<HTMLDivElement>(null)
  const [room, setRoom] = useState(0),
    [refreshing, setRefreshing] = useState(false)
  const fetching = useIsFetching({ queryKey: ['moments', 'entries'] })
  const refreshAnchor = useRef<Anchor | null>(null)
  const returning = useRef(false)
  const selected = visit.selected
  const narrow = room < 860
  const left = Math.max(420, Math.min(room * visit.ratio, room - 324))
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
    if (!layout.current) return
    const observer = new ResizeObserver(([entry]) => {
      if (entry.contentRect.width) setRoom(entry.contentRect.width)
    })
    observer.observe(layout.current)
    if (viewport.current) viewport.current.scrollTop = visit.scroll.current
    return () => observer.disconnect()
  }, [])
  function saveAnchor(id?: string): Anchor {
    const pane = viewport.current!
    const nodes = [...pane.querySelectorAll<HTMLElement>('[data-moment-id]')]
    const top = pane.getBoundingClientRect().top
    const node = id
      ? nodes.find((node) => node.dataset.momentId === id)
      : nodes.find((node) => node.getBoundingClientRect().bottom > top)
    return {
      id: node?.dataset.momentId ?? '',
      offset: node ? node.getBoundingClientRect().top - top : 0,
      position: pane.scrollTop,
    }
  }
  function restore(saved: Anchor, focus = false) {
    const pane = viewport.current
    if (!pane) return
    const node = [...pane.querySelectorAll<HTMLElement>('[data-moment-id]')].find(
      (node) => node.dataset.momentId === saved.id,
    )
    pane.scrollTop = saved.position
    if (node) {
      pane.scrollTop +=
        node.getBoundingClientRect().top - pane.getBoundingClientRect().top - saved.offset
      if (focus)
        node.querySelector<HTMLButtonElement>('.moment-summary')?.focus({ preventScroll: true })
    }
  }
  useLayoutEffect(() => {
    if (!selected && returning.current) {
      if (visit.origin.current) restore(visit.origin.current, true)
      returning.current = false
    }
    if (refreshing || fetching || !refreshAnchor.current || selected) return
    restore(refreshAnchor.current)
    refreshAnchor.current = null
  }, [selected, refreshing, fetching])
  function open(entry: MomentEntry) {
    visit.origin.current = saveAnchor(entry.id)
    visit.setSelected(entry)
  }
  function close() {
    returning.current = true
    visit.setSelected(null)
  }
  useEffect(() => {
    if (!selected) return
    const onKey = (event: KeyboardEvent) => {
      if (
        event.key !== 'Escape' ||
        event.defaultPrevented ||
        document.querySelector(
          '[role="dialog"], [role="menu"], [data-radix-popper-content-wrapper]',
        )
      )
        return
      event.preventDefault()
      close()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [selected])
  async function refresh() {
    if (refreshing) return
    refreshAnchor.current = saveAnchor()
    setRefreshing(true)
    try {
      const result = await days.refetch()
      if (result.error) throw result.error
      await Promise.all([
        client.refetchQueries(
          { queryKey: ['moments', 'entries', result.data?.pages[0].timezone], type: 'all' },
          { throwOnError: true },
        ),
        client.refetchQueries(
          { queryKey: ['moments', 'overview'], type: 'active' },
          { throwOnError: true },
        ),
      ])
    } catch (error) {
      toast((error as Error).message)
    } finally {
      setRefreshing(false)
    }
  }
  const yesterday = first ? new Date(`${first.today}T12:00:00Z`) : null
  yesterday?.setUTCDate(yesterday.getUTCDate() - 1)
  const yesterdayKey = yesterday?.toISOString().slice(0, 10)
  return (
    <div className="moments-page">
      <PageBreadcrumbs
        label={
          narrow && !selected ? (
            <span className="moments-view-switch">
              <ChoicePicker
                label={t('moments.switchView')}
                value={visit.view}
                onChange={visit.setView}
                options={[
                  { value: 'feed', label: t('moments.latest') },
                  { value: 'dashboard', label: t('moments.overview') },
                ]}
              />
            </span>
          ) : (
            t('Moments')
          )
        }
      />
      {!selected && (
        <PageActions>
          <IconButton
            label={t('刷新动态')}
            onClick={refresh}
            loading={refreshing}
            disabled={days.isFetching}
          >
            <ActionIcon name="refresh" />
          </IconButton>
        </PageActions>
      )}
      <div
        ref={layout}
        hidden={!!selected}
        className={`moments-layout${narrow ? ' is-narrow' : ''}`}
        style={{ '--moments-feed-width': `${left}px` } as CSSProperties}
      >
        <div
          className="moments-feed"
          ref={viewport}
          hidden={narrow && visit.view !== 'feed'}
          onWheel={() => {
            refreshAnchor.current = null
          }}
          onTouchStart={() => {
            refreshAnchor.current = null
          }}
          onScroll={(event) => {
            if (!selected && (!narrow || visit.view === 'feed'))
              visit.scroll.current = event.currentTarget.scrollTop
          }}
        >
          {days.isPending && <p className="quiet text-muted moments-state">{t('正在加载…')}</p>}
          {days.error && <RequestError error={days.error} retry={() => days.refetch()} />}
          {first?.total_runs === 0 && (
            <p className="quiet text-muted moments-state">{t('暂无执行结果')}</p>
          )}
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
                            ? t('今天')
                            : day.date === yesterdayKey
                              ? t('昨天')
                              : day.date}
                        </h2>
                        <span>{t('moments.runCount', { count: day.count })}</span>
                      </div>
                    </div>
                    <DayEntries date={day.date} timezone={first.timezone} onOpen={open} />
                  </section>
                ))}
              {days.hasNextPage && (
                <Button
                  className="moments-load"
                  loading={days.isFetchingNextPage}
                  onClick={() => days.fetchNextPage()}
                >
                  {t('加载更早')}
                </Button>
              )}
            </div>
          )}
        </div>
        {!narrow && (
          <PaneResizeHandle
            label={t('moments.resizeFeed')}
            width={left + 12}
            min={432}
            max={room - 312}
            style={{ left: `${left + 8}px` }}
            onResize={(width) => visit.setRatio((width - 12) / room)}
            onReset={() => visit.setRatio(0.65)}
          />
        )}
        <div className="moments-dashboard" hidden={narrow && visit.view !== 'dashboard'}>
          <MomentsOverview />
        </div>
      </div>
      {selected && (
        <MomentReader key={selected.id} entry={selected} toast={toast} onClose={close} />
      )}
    </div>
  )
}
function RequestError({ error, retry }: { error: Error; retry: () => unknown }) {
  const { t } = useTranslation()
  return (
    <ErrorToast
      error={error.message}
      actions={
        <Button onClick={() => retry()}>
          <ActionIcon name="refresh" />
          {t('重试')}
        </Button>
      }
    />
  )
}
function DayEntries({
  date,
  timezone,
  onOpen,
}: {
  date: string
  timezone: string
  onOpen: (entry: MomentEntry) => void
}) {
  const { t } = useTranslation()
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
      {entries.isPending && <p className="quiet text-muted moments-state">{t('正在加载…')}</p>}
      {entries.error && <RequestError error={entries.error} retry={() => entries.refetch()} />}
      {entries.data?.pages
        .flatMap((page) => page.items)
        .map((entry) => (
          <MomentRow key={entry.id} entry={entry} timezone={timezone} onOpen={onOpen} />
        ))}
      {entries.hasNextPage && (
        <Button
          className="moments-load"
          loading={entries.isFetchingNextPage}
          onClick={() => entries.fetchNextPage()}
        >
          {t('加载更多')}
        </Button>
      )}
    </>
  )
}
