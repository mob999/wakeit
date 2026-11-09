import { type Agent, type Runner } from '../api/types'
import { SessionPicker } from './SessionPicker'

export function SessionBinding({
  runner,
  agent,
  agentInstance,
  cwd,
  value,
  disabled,
  onChange,
}: {
  runner?: Runner
  agent?: Agent
  agentInstance?: string | null
  cwd: string
  value: { id: string } | null
  disabled: boolean
  onChange: (value: { id: string } | null) => void
}) {
  return (
    <SessionPicker
      key={`${runner?.id}/${agent?.id}/${agent?.digest}/${agentInstance ?? agent?.instance_id}/${cwd}`}
      runner={runner}
      agent={agent}
      agentInstance={agentInstance}
      cwd={cwd}
      id={value?.id ?? ''}
      disabled={disabled}
      onChange={(id) => onChange(id ? { id } : null)}
    />
  )
}
