import { useRef, type ReactNode } from 'react'
import './DetailTabs.css'

export function DetailTabs<T extends string>({
  idPrefix,
  label,
  tabs,
  value,
  onChange,
  status,
  className = '',
  disabled = false,
}: {
  idPrefix: string
  label: string
  tabs: readonly { key: T; label: ReactNode; describedBy?: string }[]
  value: T
  onChange: (value: T) => void
  status?: ReactNode
  className?: string
  disabled?: boolean
}) {
  const list = useRef<HTMLDivElement>(null)
  return (
    <div className={`detail-tabs ${className}`}>
      <div className="tabs" role="tablist" aria-label={label} ref={list}>
        {tabs.map((tab, index) => (
          <button
            key={tab.key}
            id={`${idPrefix}-${tab.key}-tab`}
            type="button"
            disabled={disabled}
            role="tab"
            aria-selected={value === tab.key}
            aria-controls={`${idPrefix}-${tab.key}-panel`}
            aria-describedby={tab.describedBy}
            tabIndex={value === tab.key ? 0 : -1}
            className={value === tab.key ? 'active' : ''}
            onClick={() => onChange(tab.key)}
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
              onChange(tabs[next].key)
              list.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus()
            }}
          >
            {tab.label}
          </button>
        ))}
      </div>
      {status}
    </div>
  )
}
