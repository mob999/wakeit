import { type Agent, type Runner } from '../api'
import { SessionPicker } from './SessionPicker'

export function SessionBinding({
  runner,
  agent,
  cwd,
  value,
  disabled,
  onChange,
}: {
  runner?: Runner
  agent?: Agent
  cwd: string
  value: { id: string } | null
  disabled: boolean
  onChange: (value: { id: string } | null) => void
}) {
  return (
    <SessionPicker
      key={`${runner?.id}/${agent?.id}/${agent?.digest}/${cwd}`}
      runner={runner}
      agent={agent}
      cwd={cwd}
      id={value?.id ?? ''}
      disabled={disabled}
      onChange={(id) => onChange(id ? { id } : null)}
    />
  )
}
