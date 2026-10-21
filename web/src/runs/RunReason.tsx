import type { ReactNode } from 'react'
import * as Tooltip from '@radix-ui/react-tooltip'
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
    <Tooltip.Root>
      <Tooltip.Trigger asChild>
        <button
          type="button"
          className="run-reason"
          aria-label={accessibleLabel ?? `${label}: ${reason}`}
        >
          {children ?? label}
        </button>
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content
          className="tooltip run-reason-tooltip"
          align="end"
          sideOffset={6}
          collisionPadding={8}
        >
          {reason}
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}
