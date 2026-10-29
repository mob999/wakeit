import {
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from 'react'
import * as Popover from '@radix-ui/react-popover'
import { ListTree } from '../components/icons'
import { useTranslation } from 'react-i18next'
import { workspaceId } from '../api/client'
import { Button } from '../components/ui'
import { PaneResizeHandle } from '../components/PaneResizeHandle'
import { Status } from '../components/Status'
import { RunReason } from '../runs/RunReason'
import { ResultReferences } from '../runs/ResultReferences'
import type { FileSource } from '../files/links'
import { PreviewActivity } from '../files/PreviewActivity'
import { reportIndex, type ReportHeading, type ReportLink } from '../moments/reportModel'
import { useReferenceCapacity } from '../moments/useReferenceCapacity'
import { useReportLayout, useReportReading } from './ReportVisit'
import { defaultReportWidths, reportWidthLimits, reportWidths } from './layout'
import type { ReportReadingState } from './readingState'
import './reports.css'

export type ReportOutput = { node_id: string; name: string; state: string; reason?: string | null }
function scrollParent(element: HTMLElement): HTMLElement {
  for (let parent = element.parentElement; parent; parent = parent.parentElement) {
    if (/(auto|scroll)/.test(getComputedStyle(parent).overflowY)) return parent
  }
  return document.scrollingElement as HTMLElement
}

export function ReportReader({
  identity,
  indexKey,
  markdown,
  source,
  cwd,
  outputs = [],
  selectedOutput,
  onSelectOutput,
  preview = false,
  incomplete = false,
  onExpand,
  notice,
  onOverflowChange,
  children,
}: {
  identity: string
  indexKey: string
  markdown: boolean
  source?: FileSource
  cwd?: string
  outputs?: ReportOutput[]
  selectedOutput?: string
  onSelectOutput?: (id: string) => void
  preview?: boolean
  incomplete?: boolean
  onExpand?: () => void
  notice?: ReactNode
  onOverflowChange?: (overflow: boolean) => void
  children: ReactNode
}) {
  const { t } = useTranslation()
  const key = `${workspaceId ?? ''}:${identity}`
  const { state, update } = useReportReading(key)
  const layout = useReportLayout(workspaceId ?? '')
  const grid = useRef<HTMLDivElement>(null)
  const [room, setRoom] = useState(0)
  const reading = useRef<HTMLDivElement>(null),
    report = useRef<HTMLDivElement>(null)
  const [index, setIndex] = useState<{
    identity: string
    indexKey: string
    headings: ReportHeading[]
    links: ReportLink[]
  }>({ identity: '', indexKey: '', headings: [], links: [] })
  const [height, setHeight] = useState(162)
  const [viewportHeight, setViewportHeight] = useState<number | null>(null)
  const [navigationOpen, setNavigationOpen] = useState(false)
  useLayoutEffect(() => {
    if (!grid.current) return
    const observer = new ResizeObserver(([entry]) => setRoom(entry.contentRect.width))
    observer.observe(grid.current)
    return () => observer.disconnect()
  }, [])
  const pendingHeading = useRef<{
    identity: string
    heading: string
    node?: string
    unresolved: boolean
  } | null>(null)
  const pendingOutput = useRef<string | null>(null)
  const suppressReturnFocus = useRef(false)
  const pin = useContext(PreviewActivity),
    pinId = useId()
  useEffect(() => {
    pin(pinId, navigationOpen)
    return () => pin(pinId, false)
  }, [pin, pinId, navigationOpen])
  useLayoutEffect(() => {
    setIndex({
      identity,
      indexKey,
      ...(markdown && report.current
        ? reportIndex(report.current, cwd)
        : { headings: [], links: [] }),
    })
  }, [key, indexKey, markdown, cwd])
  useLayoutEffect(() => {
    const body = reading.current,
      content = report.current
    if (!body || !content) return
    const viewport = scrollParent(body)
    const measure = () => {
      onOverflowChange?.(content.scrollHeight > 138)
      setHeight(body.getBoundingClientRect().height)
      setViewportHeight(viewport.clientHeight)
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(body)
    observer.observe(content)
    observer.observe(viewport)
    if (content.firstElementChild) observer.observe(content.firstElementChild)
    window.addEventListener('resize', measure)
    return () => {
      observer.disconnect()
      window.removeEventListener('resize', measure)
    }
  }, [key, indexKey, preview, onOverflowChange])
  function target(heading: string) {
    const item = index.headings.find((candidate) => candidate.key === heading)
    return item
      ? report.current?.querySelectorAll<HTMLElement>('h1,h2,h3,h4,h5,h6')[item.index]
      : undefined
  }
  function remember() {
    const body = report.current
    if (preview || !body) return
    const parent = scrollParent(body),
      bounds = parent.getBoundingClientRect(),
      bodyBounds = body.getBoundingClientRect()
    if (bodyBounds.bottom <= bounds.top || bodyBounds.top >= bounds.bottom) return
    let active: ReportHeading | undefined
    for (const heading of index.headings) {
      const element = target(heading.key)
      if (element && element.getBoundingClientRect().top <= bounds.top + 64) active = heading
    }
    const element = active ? target(active.key) : body
    update({
      activeHeading: active?.key ?? null,
      anchor: {
        heading: active?.key ?? null,
        offset: (element ?? body).getBoundingClientRect().top - bounds.top,
      },
    })
  }
  useEffect(() => {
    const body = report.current
    if (!body || preview) return
    const parent = scrollParent(body)
    let frame = 0
    const onScroll = () => {
      if (!frame)
        frame = requestAnimationFrame(() => {
          frame = 0
          remember()
        })
    }
    parent.addEventListener('scroll', onScroll, { passive: true })
    return () => {
      parent.removeEventListener('scroll', onScroll)
      cancelAnimationFrame(frame)
    }
  }, [key, preview, index.headings])
  useLayoutEffect(() => {
    if (preview || incomplete || index.identity !== identity || index.indexKey !== indexKey) return
    const pending = pendingHeading.current
    if (
      pending &&
      (pending.identity === identity || (pending.unresolved && pending.node === selectedOutput))
    ) {
      const element = target(pending.heading)
      if (element) {
        element.tabIndex = -1
        element.scrollIntoView({ block: 'start', inline: 'nearest' })
        element.focus({ preventScroll: true })
        update({ activeHeading: pending.heading })
      }
      pendingHeading.current = null
    }
    if (pendingOutput.current === selectedOutput && report.current) {
      const parent = scrollParent(report.current),
        anchor = state.anchor
      const element = anchor?.heading ? target(anchor.heading) : report.current
      parent.scrollTop +=
        (element ?? report.current).getBoundingClientRect().top -
        parent.getBoundingClientRect().top -
        (anchor?.offset ?? 12)
      pendingOutput.current = null
    }
  }, [identity, selectedOutput, preview, incomplete, index])
  function select(id: string) {
    if (id === selectedOutput) return
    remember()
    pendingHeading.current = null
    pendingOutput.current = preview ? null : id
    onSelectOutput?.(id)
    setNavigationOpen(false)
  }
  function navigate(heading: string) {
    pendingHeading.current = {
      identity,
      heading,
      node: selectedOutput,
      unresolved: !!source?.nodeId,
    }
    suppressReturnFocus.current = true
    setNavigationOpen(false)
    if (preview) onExpand?.()
    else {
      const element = target(heading)
      if (element) {
        element.tabIndex = -1
        element.scrollIntoView({ block: 'start', inline: 'nearest' })
        element.focus({ preventScroll: true })
        update({ activeHeading: heading })
      }
      pendingHeading.current = null
    }
  }
  const hasNavigation = outputs.length > 0 || index.headings.length > 1
  const { widths, max } = reportWidths(room, hasNavigation, index.links.length > 0, layout.widths)
  function resize(side: 'navigation' | 'references', value: number) {
    layout.update({ ...widths, [side]: value })
  }
  const navigationProps = {
    outputs,
    selectedOutput,
    onSelect: select,
    headings: index.headings.length > 1 ? index.headings : [],
    onHeading: navigate,
    state,
    update,
    reading,
    incomplete,
  }
  return (
    <div
      className={`report-reader${preview ? ' is-preview' : ''}`}
      style={
        {
          '--report-reading-height': `${Math.max(0, height)}px`,
          '--report-viewport-height': viewportHeight == null ? undefined : `${viewportHeight}px`,
          '--report-navigation-width': `${widths.navigation}px`,
          '--report-references-width': `${widths.references}px`,
        } as CSSProperties
      }
    >
      <div
        ref={grid}
        className={`report-grid${hasNavigation ? ' has-navigation' : ''}${index.links.length ? ' has-references' : ''}`}
      >
        {hasNavigation && (
          <>
            <ReportNavigation {...navigationProps} preview={preview} />
            <PaneResizeHandle
              className="report-navigation-resize"
              label={t('report.resizeNavigation')}
              width={widths.navigation}
              min={reportWidthLimits.navigation.min}
              max={max.navigation}
              onResize={(value) => resize('navigation', value)}
              onReset={() => resize('navigation', defaultReportWidths.navigation)}
              style={{ left: 'calc(var(--report-navigation-width) - 4px)' }}
            />
            <div className="report-tools">
              <Popover.Root open={navigationOpen} onOpenChange={setNavigationOpen}>
                <Popover.Trigger asChild>
                  <Button className="report-navigation-trigger">
                    <ListTree size={14} />
                    {t('report.navigation')}
                  </Button>
                </Popover.Trigger>
                <Popover.Portal>
                  <Popover.Content
                    className="report-nav-popover"
                    align="start"
                    sideOffset={6}
                    collisionPadding={8}
                    onCloseAutoFocus={(event) => {
                      if (suppressReturnFocus.current) event.preventDefault()
                      suppressReturnFocus.current = false
                    }}
                  >
                    <ReportNavigation {...navigationProps} preview={false} popup />
                  </Popover.Content>
                </Popover.Portal>
              </Popover.Root>
              {outputs.find((output) => output.node_id === selectedOutput)?.name && (
                <span className="report-current-output">
                  {outputs.find((output) => output.node_id === selectedOutput)?.name}
                </span>
              )}
            </div>
          </>
        )}
        <div className="report-reading-body" ref={reading}>
          {notice}
          <div className={`report-content${preview ? ' report-content-preview' : ''}`} ref={report}>
            {children}
          </div>
        </div>
        {index.links.length > 0 && (
          <>
            <PaneResizeHandle
              className="report-references-resize"
              label={t('report.resizeReferences')}
              edge="right"
              width={widths.references}
              min={reportWidthLimits.references.min}
              max={max.references}
              onResize={(value) => resize('references', value)}
              onReset={() => resize('references', defaultReportWidths.references)}
              style={{ right: 'calc(var(--report-references-width) - 4px)' }}
            />
            <ResultReferences
              links={index.links}
              reading={reading}
              source={source}
              expanded={state.referencesExpanded}
              onExpandedChange={(value) => update({ referencesExpanded: value })}
              scroll={state.referencesScroll}
              onScroll={(value) => update({ referencesScroll: value })}
              identity={key}
              preview={preview}
            />
          </>
        )}
      </div>
    </div>
  )
}

function ReportNavigation({
  outputs,
  selectedOutput,
  onSelect,
  headings,
  onHeading,
  state,
  update,
  reading,
  preview,
  incomplete,
  popup = false,
}: {
  outputs: ReportOutput[]
  selectedOutput?: string
  onSelect: (id: string) => void
  headings: ReportHeading[]
  onHeading: (key: string) => void
  state: ReportReadingState
  update: (patch: Partial<ReportReadingState>) => void
  reading: RefObject<HTMLDivElement | null>
  preview: boolean
  incomplete: boolean
  popup?: boolean
}) {
  const { t } = useTranslation()
  const context = useRef<HTMLElement>(null),
    list = useRef<HTMLElement>(null),
    toggle = useRef<HTMLButtonElement>(null)
  const capacity = useReferenceCapacity({
    reading,
    context,
    list,
    toggle,
    count: preview ? headings.length : 0,
  })
  const collapsed = preview && !state.outlineExpanded
  useLayoutEffect(() => {
    if (list.current) list.current.scrollTop = state.outlineScroll
  }, [headings, popup])
  const depth = Math.min(...headings.map((heading) => heading.depth))
  return (
    <aside
      className={`report-navigation${popup ? ' report-navigation-popup' : ''}`}
      ref={context}
      aria-label={t('report.navigation')}
    >
      {outputs.length > 0 && (
        <section className="report-output-section">
          <h4>{t('moments.outputTitle')}</h4>
          <div className="report-output-list">
            {outputs.map((output) =>
              outputs.length === 1 ? (
                <div className="report-output-row" key={output.node_id} title={output.name}>
                  <span>{output.name}</span>
                  <RunReason state={output.state} reason={output.reason}>
                    <Status state={output.state} />
                  </RunReason>
                </div>
              ) : (
                <div
                  key={output.node_id}
                  className={`report-output-row${selectedOutput === output.node_id ? ' selected' : ''}`}
                >
                  <button
                    type="button"
                    className="report-output-select"
                    aria-pressed={selectedOutput === output.node_id}
                    aria-label={`${output.name} · ${output.node_id}`}
                    title={`${output.name} · ${output.node_id}`}
                    onClick={() => onSelect(output.node_id)}
                  >
                    <span>
                      {output.name}
                      {outputs.some(
                        (other) => other.node_id !== output.node_id && other.name === output.name,
                      ) && <small>{output.node_id.slice(0, 8)}</small>}
                    </span>
                  </button>
                  <RunReason state={output.state} reason={output.reason}>
                    <Status state={output.state} />
                  </RunReason>
                </div>
              ),
            )}
          </div>
        </section>
      )}
      {headings.length > 0 && (
        <section className="report-outline-section">
          <h4>
            {t('moments.outline')}
            {incomplete && <span className="quiet text-muted">{t('moments.excerpt')}</span>}
          </h4>
          <nav
            className="report-outline-list"
            aria-label={t('moments.outline')}
            ref={list}
            onScroll={(event) => update({ outlineScroll: event.currentTarget.scrollTop })}
          >
            {(collapsed ? headings.slice(0, capacity) : headings).map((heading) => (
              <button
                type="button"
                key={heading.key}
                aria-current={state.activeHeading === heading.key ? 'location' : undefined}
                title={heading.label}
                style={{ paddingInlineStart: `${6 + Math.min(3, heading.depth - depth) * 10}px` }}
                onClick={() => onHeading(heading.key)}
              >
                {heading.label}
              </button>
            ))}
          </nav>
          {preview && (state.outlineExpanded || headings.length > capacity) && (
            <button
              type="button"
              className="report-more"
              ref={toggle}
              aria-expanded={state.outlineExpanded}
              onClick={() => update({ outlineExpanded: !state.outlineExpanded })}
            >
              {state.outlineExpanded
                ? t('收起')
                : t('moments.more', { count: headings.length - capacity })}
            </button>
          )}
        </section>
      )}
    </aside>
  )
}
