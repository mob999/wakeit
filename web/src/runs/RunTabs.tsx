import { useCallback, useEffect, type ReactNode, type Ref } from 'react'
import { useTranslation } from 'react-i18next'
import { useRunReading } from './RunReading'
import { runTab, type RunTab } from './readingModel'
import './RunTabs.css'

const tabs = ['output', 'trace', 'details'] as const

export function useRunView(id: string, outputAvailable: boolean) {
  const [reading, updateReading] = useRunReading(id)
  const tab = runTab(reading, outputAvailable)
  useEffect(() => {
    if (outputAvailable && !reading.highlighted)
      updateReading({ highlighted: true, resultReady: tab !== 'output', viewTab: tab })
  }, [outputAvailable, reading.highlighted, tab, updateReading])
  const setTab = useCallback(
    (viewTab: RunTab) =>
      updateReading({ viewTab, ...(viewTab === 'output' ? { resultReady: false } : {}) }),
    [updateReading],
  )
  return { reading, updateReading, tab, setTab }
}

export function RunTabs({
  id,
  tab,
  onChange,
  outputReady,
  status,
}: {
  id: string
  tab: RunTab
  onChange: (tab: RunTab) => void
  outputReady: boolean
  status?: ReactNode
}) {
  const { t } = useTranslation()
  return (
    <div className="run-tabs">
      <div className="tabs" role="tablist" aria-label={t('运行内容')}>
        {tabs.map((key, index) => (
          <button
            key={key}
            id={`run-${id}-${key}-tab`}
            type="button"
            role="tab"
            aria-selected={tab === key}
            aria-controls={`run-${id}-${key}-panel`}
            aria-describedby={key === 'output' && outputReady ? `run-${id}-ready` : undefined}
            tabIndex={tab === key ? 0 : -1}
            className={tab === key ? 'active' : ''}
            onClick={() => onChange(key)}
            onKeyDown={(event) => {
              const next =
                event.key === 'ArrowRight'
                  ? (index + 1) % tabs.length
                  : event.key === 'ArrowLeft'
                    ? (index + tabs.length - 1) % tabs.length
                    : event.key === 'Home'
                      ? 0
                      : event.key === 'End'
                        ? tabs.length - 1
                        : -1
              if (next < 0) return
              event.preventDefault()
              onChange(tabs[next])
              document.getElementById(`run-${id}-${tabs[next]}-tab`)?.focus()
            }}
          >
            {t(key === 'details' ? '详情' : `run.${key}`)}
            {key === 'output' && outputReady && (
              <span className="run-output-ready" aria-hidden="true" />
            )}
          </button>
        ))}
      </div>
      {outputReady && (
        <span id={`run-${id}-ready`} className="sr-only" role="status">
          {t('reading.resultReady')}
        </span>
      )}
      {status}
    </div>
  )
}

export function RunPanel({
  id,
  tab,
  active,
  className = '',
  children,
  ref,
}: {
  id: string
  tab: RunTab
  active: RunTab
  className?: string
  children: ReactNode
  ref?: Ref<HTMLDivElement>
}) {
  return (
    <div
      id={`run-${id}-${tab}-panel`}
      role="tabpanel"
      aria-labelledby={`run-${id}-${tab}-tab`}
      tabIndex={0}
      className={`run-page ${className}`}
      hidden={active !== tab}
      ref={ref}
    >
      {children}
    </div>
  )
}
