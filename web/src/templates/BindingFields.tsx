import { useState } from 'react'
import { t } from '../i18n'
import { ChoicePicker } from '../ChoicePicker'
import { ModelPicker, ParameterFields } from '../ModelPicker'
import { Field, Button } from '../ui'
import type { Runner } from '../api'
import type { Binding, TemplateIssue } from './types'

export function BindingFields({
  title,
  slot,
  defaults,
  value,
  runners,
  directories,
  onChange,
}: {
  title: string
  slot: string
  defaults: Binding
  value: Binding
  runners: Runner[]
  directories: string[]
  issues: TemplateIssue[]
  onChange: (value: Binding) => void
}) {
  const [options, setOptions] = useState<Record<string, string[]>>({})
  const [directory, setDirectory] = useState('')
  const cwd =
    directories.length === 1 ? directories[0] : directories.includes(directory) ? directory : ''
  const runnerId = value.runner ?? defaults.runner ?? '',
    agentId = value.agent ?? defaults.agent ?? '',
    parameters = value.parameters ?? defaults.parameters ?? {}
  const runner = runners.find((runner) => runner.id === runnerId || runner.name === runnerId),
    agent = runner?.agents.find((agent) => agent.id === agentId)
  return (
    <details
      open={!runner || !agent ? true : undefined}
      className="template-binding"
      data-input-path={`bindings.${slot}`}
    >
      <summary>
        <span>{title}</span>
        <span className="template-binding-summary">
          <span>
            <span className="quiet">{t('设备')}</span>
            <span>{runner?.name || t('选择设备')}</span>
          </span>
          <span>
            <span className="quiet">Agent</span>
            <span>{agent?.name || t('选择 Agent')}</span>
          </span>
        </span>
      </summary>
      <div className="template-binding-body">
        {Object.keys(value).length > 0 && (
          <div className="template-binding-heading">
            <Button onClick={() => onChange({})}>{t('creation.resetDefault')}</Button>
          </div>
        )}
        <div data-input-path={`bindings.${slot}.runner`}>
          <Field label={t('设备')}>
            <ChoicePicker
              label={t('taskTemplate.runnerBinding', { name: title })}
              value={runner?.id ?? runnerId}
              searchable
              onChange={(runner) => onChange({ runner, agent: '', parameters: {} })}
              options={runners.map((runner) => ({
                value: runner.id,
                label: runner.name || runner.id,
                status: runner.online ? undefined : t('离线'),
              }))}
            />
          </Field>
        </div>
        <div data-input-path={`bindings.${slot}.agent`}>
          <Field label="Agent">
            <ChoicePicker
              label={t('taskTemplate.agentBinding', { name: title })}
              value={agentId}
              disabled={!runner}
              onChange={(agent) => onChange({ ...value, agent, parameters: {} })}
              options={(runner?.agents ?? []).map((agent) => ({
                value: agent.id,
                label: agent.name,
                disabled: !agent.available,
              }))}
            />
          </Field>
        </div>
        {directories.length > 1 && agent?.models_supported && (
          <Field label={t('creation.modelDirectory')}>
            <ChoicePicker
              label={t('creation.modelDirectory')}
              value={cwd}
              searchable
              onChange={setDirectory}
              options={directories.map((path) => ({ value: path, label: path }))}
            />
          </Field>
        )}
        <div data-input-path={`bindings.${slot}.parameters`}>
          <ModelPicker
            allowManualWithoutDirectory
            key={`${slot}-${runnerId}-${agentId}`}
            runner={runner}
            agent={agent}
            cwd={cwd}
            values={parameters}
            onChange={(parameters) => onChange({ ...value, parameters })}
            onOptionsChange={setOptions}
          />
          <details className="template-binding-advanced">
            <summary>{t('高级设置')}</summary>
            {Object.keys(parameters).length > 0 && (
              <Button onClick={() => onChange({ ...value, parameters: {} })}>
                {t('creation.agentDefaults')}
              </Button>
            )}
            <ParameterFields
              agent={agent}
              values={parameters}
              options={options}
              onChange={(parameters) => onChange({ ...value, parameters })}
            />
          </details>
        </div>
      </div>
    </details>
  )
}
