import { ActionIcon } from '../actions/ActionIcon'
import { ChoicePicker } from '../components/ChoicePicker'
import { IconButton } from '../components/ui'

import { t } from '../i18n'
export type NodeOption = {
  id: string
  name: string
  description?: string
  keywords?: string
  disabled?: boolean
  reason?: string
}
export function MultiPicker({
  label,
  value,
  options,
  disabled = false,
  onChange,
  empty,
  minimum = 0,
  chips = false,
}: {
  label: string
  value: string[]
  options: NodeOption[]
  disabled?: boolean
  onChange: (ids: string[]) => void
  empty?: string
  minimum?: number
  chips?: boolean
}) {
  const toggle = (id: string) =>
    onChange(value.includes(id) ? value.filter((v) => v !== id) : [...value, id])
  return (
    <div className="dag-multi-picker">
      <ChoicePicker
        label={label}
        searchable
        value={value[0] ?? ''}
        selectionLabel={
          value.length
            ? value.map((id) => options.find((o) => o.id === id)?.name || id).join(', ')
            : empty || t('dag.selectNodes')
        }
        disabled={disabled}
        multiple={value}
        minimum={minimum}
        onToggle={toggle}
        onChange={() => {}}
        options={options.map((o) => ({
          value: o.id,
          label: o.name,
          description: [o.description, o.reason].filter(Boolean).join(' · '),
          keywords: o.keywords || o.id,
          disabled: o.disabled,
        }))}
      />
      {chips && value.length > 0 && (
        <div className="dag-predecessors">
          {value.map((id) => (
            <div key={id}>
              <span>{options.find((o) => o.id === id)?.name || id}</span>
              <IconButton
                label={t('dag.removePre', { name: options.find((o) => o.id === id)?.name || id })}
                disabled={disabled || value.length <= minimum}
                onClick={() => toggle(id)}
              >
                <ActionIcon name="close" />
              </IconButton>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
