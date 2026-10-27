import { useState } from 'react'
import { t } from '../i18n'
import { ChoicePicker } from '../ChoicePicker'
import { ModelPicker, ParameterFields } from '../ModelPicker'
import { Field } from '../ui'
import type { Runner } from '../api'
import type { Binding } from './types'

export function BindingFields({
  title,
  slot,
  defaults,
  value,
  runners,
  onChange,
}: {
  title: string
  slot: string
  defaults: Binding
  value: Binding
  runners: Runner[]
  onChange: (value: Binding) => void
}) {
  const [options, setOptions] = useState<Record<string, string[]>>({})
  const runnerId = value.runner ?? defaults.runner ?? '',
    agentId = value.agent ?? defaults.agent ?? '',
    parameters = value.parameters ?? defaults.parameters ?? {}
  const runner = runners.find((r) => r.id === runnerId || r.name === runnerId),
    agent = runner?.agents.find((a) => a.id === agentId)
  return (
    <div className="template-binding">
      <h4>{title}</h4>
      <Field label={t('设备')}>
        <ChoicePicker
          label={t('taskTemplate.runnerBinding', { name: title })}
          value={runnerId}
          searchable
          onChange={(runner) => onChange({ ...value, runner, agent: null, parameters: {} })}
          options={runners.map((r) => ({
            value: r.id,
            label: r.name || r.id,
            status: !r.online ? t('离线') : undefined,
          }))}
        />
      </Field>
      <Field label="Agent">
        <ChoicePicker
          label={t('taskTemplate.agentBinding', { name: title })}
          value={agentId}
          onChange={(agent) => onChange({ ...value, agent, parameters: {} })}
          options={(runner?.agents ?? []).map((a) => ({
            value: a.id,
            label: a.name,
            disabled: !a.available,
          }))}
        />
      </Field>
      <ModelPicker
        key={`${slot}-${runnerId}-${agentId}`}
        runner={runner}
        agent={agent}
        cwd={runner?.directory_roots[0] ?? ''}
        values={parameters}
        onChange={(parameters) => onChange({ ...value, parameters })}
        onOptionsChange={setOptions}
      />
      <ParameterFields
        agent={agent}
        values={parameters}
        options={options}
        onChange={(parameters) => onChange({ ...value, parameters })}
      />
    </div>
  )
}
