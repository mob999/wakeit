import { useEffect, useRef, useState, type ReactNode } from 'react'
import { PageBreadcrumbs, PageListViewContext } from './PageHeader'
import { PaneResizeHandle } from './PaneResizeHandle'

export function Split({
  list,
  children,
  detailOpen = false,
  label,
  crumbs = [],
  onBack,
}: {
  list: ReactNode
  children: ReactNode
  detailOpen?: boolean
  label: string
  crumbs?: { label: string; onClick?: () => void }[]
  onBack: () => void
}) {
  const [preferredWidth, setWidth] = useState(() =>
    Math.max(270, Math.min(560, Number(localStorage.getItem('wakeit-list-width')) || 350)),
  )
  const paneRef = useRef<HTMLDivElement>(null)
  const [room, setRoom] = useState(0)
  useEffect(() => {
    const observer = new ResizeObserver(([entry]) => setRoom(entry.contentRect.width))
    observer.observe(paneRef.current!)
    return () => observer.disconnect()
  }, [])
  const max = room ? Math.min(560, Math.max(270, room - 360)) : 560
  const width = Math.min(preferredWidth, max)
  const resize = (next: number) => {
    const value = Math.max(270, Math.min(max, next))
    setWidth(value)
    localStorage.setItem('wakeit-list-width', String(value))
  }
  return (
    <PageListViewContext.Provider value={!detailOpen}>
      <div
        ref={paneRef}
        className={`split ${detailOpen ? 'detail-open' : ''}`}
        style={{ gridTemplateColumns: `${width}px minmax(0,1fr)` }}
      >
        <section className="list-pane">{list}</section>
        <PaneResizeHandle
          label="调整列表宽度"
          width={width}
          min={270}
          max={max}
          onResize={resize}
          onReset={() => resize(350)}
          style={{ left: width - 4 }}
        />
        <section className="detail-pane">
          <PageBreadcrumbs label={label} crumbs={crumbs} onBack={onBack} />
          {children}
        </section>
      </div>
    </PageListViewContext.Provider>
  )
}

export function DetailBody({
  children,
  limited = false,
}: {
  children: ReactNode
  limited?: boolean
}) {
  return (
    <div className="detail-scroll">
      <div className={limited ? 'detail-content limited' : 'detail-content'}>{children}</div>
    </div>
  )
}
