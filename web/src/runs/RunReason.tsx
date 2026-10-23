import type { ReactNode } from 'react'
import { InfoTrigger } from '../actions/InfoTrigger'
import { t } from '../i18n'

export function RunReason({
  reason,
  state,
  label: suppliedLabel,
  children,
  accessibleLabel,
}: {
  reason?: string | null
  state: string
  label?: string
  children?: ReactNode
  accessibleLabel?: string
}) {
  if (!reason) return children ?? null
  const label =
    suppliedLabel ??
    t(
      ['failed', 'timed_out', 'interrupted'].includes(state)
        ? 'reading.failureReason'
        : 'reading.reason',
    )
  return (
    <InfoTrigger
      label={accessibleLabel ?? label}
      tooltip={reason}
      className="run-reason"
      content={<p>{reason}</p>}
    >
      {children ?? label}
    </InfoTrigger>
  )
}
