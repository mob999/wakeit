import { useId } from 'react'
import { useTranslation } from 'react-i18next'
import { Status } from '../Status'
import { t } from '../i18n'
import { TooltipTarget } from '../ui'
import { ActionIcon } from '../actions/ActionIcon'
import './run-actions.css'

export function RunNodeList({
  value,
  onChange,
  disabled,
  options,
}: {
  value: string
  onChange: (value: string) => void
  disabled?: boolean
  options: {
    value: string
    name: string
    state?: string
    reason?: string
    disabled?: boolean
    pending?: boolean
    all?: boolean
  }[]
}) {
  useTranslation()
  const id = useId()
  return (
    <div className="list-scroll run-node-list" aria-label={t('dag.selectNode')}>
      {options.map((option, index) => {
        const blocked = disabled || option.disabled || option.pending
        const reason = option.pending ? t('正在加载…') : option.reason
        return (
          <div key={option.value} className={option.all ? 'run-node-all' : undefined}>
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
                  <span>
                    {option.all && <ActionIcon name="retry" />}
                    {option.name}
                  </span>
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
  )
}
