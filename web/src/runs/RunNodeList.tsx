import { useEffect, useId, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Status } from '../Status'
import { t } from '../i18n'
import { TooltipTarget } from '../ui'
import { Pagination } from '../Pagination'
import './run-actions.css'

export const RUN_NODE_PAGE_SIZE = 8

export function RunNodeList({
  value,
  onChange,
  disabled,
  options,
  page,
  total,
  onPageChange,
}: {
  value: string
  onChange: (value: string) => void
  disabled?: boolean
  page: number
  total: number
  onPageChange: (page: number) => void
  options: {
    value: string
    name: string
    state?: string
    reason?: string
    disabled?: boolean
    pending?: boolean
  }[]
}) {
  useTranslation()
  const id = useId()
  const list = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (list.current) list.current.scrollTop = 0
  }, [page])
  return (
    <div className="run-node-picker flex flex-col flex-1 min-w-0 min-h-0">
      <div ref={list} className="list-scroll run-node-list" aria-label={t('dag.selectNode')}>
        {options.map((option, index) => {
          const blocked = disabled || option.disabled || option.pending
          const reason = option.pending ? t('正在加载…') : option.reason
          return (
            <div key={option.value}>
              <TooltipTarget label={reason || option.name}>
                <button
                  type="button"
                  className={`task-row ${value === option.value ? 'active' : ''}`}
                  aria-pressed={value === option.value}
                  aria-disabled={blocked || undefined}
                  aria-describedby={reason ? `${id}-${index}` : undefined}
                  onClick={() => {
                    if (!blocked) onChange(option.value)
                  }}
                >
                  <span className="row-title">
                    <span>{option.name}</span>
                    {option.state && <Status state={option.state} />}
                  </span>
                </button>
              </TooltipTarget>
              {reason && (
                <span className="sr-only" id={`${id}-${index}`}>
                  {reason}
                </span>
              )}
            </div>
          )
        })}
      </div>
      <Pagination
        page={page}
        total={total}
        size={RUN_NODE_PAGE_SIZE}
        onChange={onPageChange}
        pending={disabled}
      />
    </div>
  )
}
