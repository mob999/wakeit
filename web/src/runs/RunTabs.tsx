import { useCallback, useEffect, type ReactNode, type Ref } from 'react'
import { useTranslation } from 'react-i18next'
import { useRunReading } from './RunReading'
import { runTab, type RunTab } from './readingModel'
import { DetailTabs } from '../components/DetailTabs'
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
    <DetailTabs<RunTab>
      className="run-tabs"
      idPrefix={`run-${id}`}
      label={t('运行内容')}
      value={tab}
      onChange={onChange}
      tabs={tabs.map((key) => ({
        key,
        describedBy: key === 'output' && outputReady ? `run-${id}-ready` : undefined,
        label: (
          <>
            {t(key === 'details' ? '详情' : `run.${key}`)}
            {key === 'output' && outputReady && (
              <span className="run-output-ready" aria-hidden="true" />
            )}
          </>
        ),
      }))}
      status={
        <>
          {outputReady && (
            <span id={`run-${id}-ready`} className="sr-only" role="status">
              {t('reading.resultReady')}
            </span>
          )}
          {status}
        </>
      }
    />
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
