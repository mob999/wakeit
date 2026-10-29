import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { PageBreadcrumbs, PageListViewContext } from './PageHeader'
import { PaneResizeHandle } from './PaneResizeHandle'

const widthProfiles = {
  objects: { key: 'wakeit-list-width', min: 270, max: 560, initial: 350 },
  settings: { key: 'wakeit-settings-width', min: 200, max: 320, initial: 240 },
}
function storedWidth(profile: keyof typeof widthProfiles) {
  const { key, min, max, initial } = widthProfiles[profile]
  try {
    return Math.max(min, Math.min(max, Number(localStorage.getItem(key)) || initial))
  } catch {
    return initial
  }
}

export function Split({
  list,
  children,
  detailOpen = false,
  label,
  crumbs = [],
  onBack,
  widthProfile = 'objects',
}: {
  list: ReactNode
  children: ReactNode
  detailOpen?: boolean
  label: string
  crumbs?: { label: string; onClick?: () => void }[]
  onBack: () => void
  widthProfile?: keyof typeof widthProfiles
}) {
  useTranslation()
  const profile = widthProfiles[widthProfile]
  const [preferences, setWidth] = useState(() => ({
    objects: storedWidth('objects'),
    settings: storedWidth('settings'),
  }))
  const preferredWidth = preferences[widthProfile]
  const paneRef = useRef<HTMLDivElement>(null)
  const [room, setRoom] = useState(0)
  useEffect(() => {
    const observer = new ResizeObserver(([entry]) => setRoom(entry.contentRect.width))
    observer.observe(paneRef.current!)
    return () => observer.disconnect()
  }, [])
  const max = room ? Math.min(profile.max, Math.max(profile.min, room - 360)) : profile.max
  const width = Math.min(preferredWidth, max)
  const resize = (next: number) => {
    const value = Math.max(profile.min, Math.min(max, next))
    setWidth((old) => ({ ...old, [widthProfile]: value }))
    try {
      localStorage.setItem(profile.key, String(value))
    } catch {}
  }
  return (
    <PageListViewContext.Provider value={!detailOpen}>
      <div
        ref={paneRef}
        className={`split split-${widthProfile} ${detailOpen ? 'detail-open' : ''}`}
        style={{ gridTemplateColumns: `${width}px minmax(0,1fr)` }}
      >
        <section className="list-pane">{list}</section>
        <PaneResizeHandle
          label={tr('调整列表宽度')}
          width={width}
          min={profile.min}
          max={max}
          onResize={resize}
          onReset={() => resize(profile.initial)}
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
  useTranslation()
  return (
    <div className="detail-scroll">
      <div className={limited ? 'detail-content limited' : 'detail-content'}>{children}</div>
    </div>
  )
}
