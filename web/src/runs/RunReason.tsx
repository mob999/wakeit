import { useId, useState } from 'react'
import { t } from '../i18n'

export function RunReason({
  reason,
  state,
  label: suppliedLabel,
}: {
  reason?: string | null
  state: string
  label?: string
}) {
  const [open, setOpen] = useState(false),
    id = useId()
  if (!reason) return null
  const label =
    suppliedLabel ??
    t(
      ['failed', 'timed_out', 'interrupted'].includes(state)
        ? 'reading.failureReason'
        : 'reading.reason',
    )
  return (
    <>
      <button
        type="button"
        className="run-reason"
        aria-label={`${label}: ${reason}`}
        title={reason}
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen((v) => !v)}
      >
        <span>{label}</span>
        <span>{reason}</span>
      </button>
      {open && (
        <div id={id} className="run-reason-full">
          {reason}
        </div>
      )}
    </>
  )
}
