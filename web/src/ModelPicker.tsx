import { ActionIcon } from './actions/ActionIcon'
import { ErrorToast } from './Toast'
import { EditorMemoryContext, useEditorValue } from './EditorMemory'
import { useContext } from 'react'
import { t as tr, parameterLabel } from './i18n'
import { useTranslation } from 'react-i18next'
import { useId, useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'
import { ChevronDown, Loader2 } from './icons'
import { api, type Agent, type Runner, type ModelCatalog } from './api'
import { IconButton, Field } from './ui'
import { ChoicePicker } from './ChoicePicker'
const EMPTY_OPTIONS: Record<string, string[]> = {}
export function ModelPicker({
  runner,
  agent,
  cwd,
  values,
  onChange,
  onOptionsChange,
  submitting = false,
}: {
  submitting?: boolean
  runner?: Runner
  agent?: Agent
  cwd: string
  values: Record<string, string>
  onChange: (v: Record<string, string>) => void
  onOptionsChange: (v: Record<string, string[]>) => void
}) {
  useTranslation()
  const id = useId()
  const memory = useContext(EditorMemoryContext)
  const [custom, setCustom] = useEditorValue('model.custom', false)
  const catalog = useQuery<ModelCatalog>({
    queryKey: ['models', runner?.id, agent?.id, cwd, agent?.digest],
    enabled: memory?.active !== false && !!(runner?.online && agent?.models_supported && cwd),
    queryFn: () =>
      api(`/runners/${runner!.id}/agents/${encodeURIComponent(agent!.id)}/models/query`, 'POST', {
        cwd,
      }),
    retry: false,
    refetchOnWindowFocus: false,
    gcTime: memory ? 300000 : 0,
    staleTime: memory ? 30000 : 0,
  })
  const models =
    runner?.online && cwd && !catalog.isError && !catalog.isFetching
      ? (catalog.data?.models ?? [])
      : []
  const selected = models.find((m) =>
    Object.entries(m.parameters).every(([k, v]) => values[k] === v),
  )
  useEffect(
    () => onOptionsChange(selected?.parameter_options ?? EMPTY_OPTIONS),
    [selected?.parameter_options, onOptionsChange],
  )
  const ready = !!agent?.models_supported && !!runner?.online && !!cwd && !catalog.isFetching
  const waiting = !!agent?.models_supported && (!cwd || catalog.isFetching)
  const disabled = !agent || waiting
  const manual =
    !!agent?.parameters?.model &&
    !waiting &&
    (!models.length || custom || (!selected && !!values.model))
  const caption = !agent
    ? tr('先选择 Agent')
    : !cwd && agent.models_supported
      ? tr('先选择工作目录')
      : catalog.isFetching
        ? tr('正在查询模型…')
        : (selected?.name ?? (values.model ? tr('自定义模型') : tr('使用默认配置')))
  function choose(modelId: string) {
    if (modelId === '__custom') {
      setCustom(true)
      return
    }
    setCustom(false)
    const next = { ...values }
    for (const key of new Set([
      'model',
      'provider',
      ...models.flatMap((m) => Object.keys(m.parameters)),
    ]))
      delete next[key]
    if (modelId !== '__default')
      Object.assign(next, models.find((m) => m.id === modelId)!.parameters)
    onChange(next)
  }
  return (
    <div className="field model-field" data-field="parameters.model">
      <label htmlFor={id}>{tr('模型')}</label>
      <div className="model-control">
        {models.length && !custom ? (
          <ChoicePicker
            id={id}
            label={tr('模型')}
            searchable
            disabled={submitting}
            value={selected?.id ?? '__default'}
            onChange={choose}
            options={[
              { value: '__default', label: tr('使用默认配置') },
              ...models.map((m) => ({
                value: m.id,
                label: m.name,
                description: m.id,
                keywords: m.id,
              })),
              { value: '__custom', label: tr('自定义模型 ID') },
            ]}
          />
        ) : manual ? (
          <input
            id={id}
            aria-label={tr('模型')}
            className="mono"
            disabled={submitting}
            value={values.model ?? ''}
            placeholder={agent?.parameters.model?.default ?? tr('Agent 默认')}
            onChange={(e) => {
              const next = { ...values }
              if (e.target.value) next.model = e.target.value
              else delete next.model
              onChange(next)
            }}
          />
        ) : (
          <button
            type="button"
            id={id}
            aria-label={tr('模型')}
            role="combobox"
            aria-expanded={false}
            disabled={submitting || disabled || !models.length}
            className="model-selector"
          >
            <span>{caption}</span>
            {catalog.isFetching ? (
              <Loader2 size={14} className="spin" />
            ) : (
              <ChevronDown size={14} />
            )}
          </button>
        )}
        {agent?.models_supported && (
          <IconButton
            type="button"
            label={tr('刷新模型')}
            loading={catalog.isFetching}
            disabled={submitting || !ready}
            onClick={() => {
              setCustom(false)
              catalog.refetch()
            }}
          >
            <ActionIcon name="refresh" />
          </IconButton>
        )}
        {custom && models.length > 0 && (
          <IconButton
            type="button"
            label={tr('返回模型列表')}
            disabled={submitting}
            onClick={() => setCustom(false)}
          >
            <ActionIcon name="list" />
          </IconButton>
        )}
      </div>
      {catalog.error ? (
        <ErrorToast>
          {catalog.error.message}
          {tr('，可手填或重试。')}
        </ErrorToast>
      ) : agent?.models_supported && catalog.data && !catalog.isFetching && !models.length ? (
        <small>{tr('未返回可选模型，可手填。')}</small>
      ) : !runner?.online && agent?.models_supported ? (
        <small>{tr('设备离线，可手填模型。')}</small>
      ) : null}
    </div>
  )
}
export function ParameterFields({
  agent,
  values,
  options,
  onChange,
  disabled = false,
}: {
  disabled?: boolean
  agent?: Agent
  values: Record<string, string>
  options: Record<string, string[]>
  onChange: (v: Record<string, string>) => void
}) {
  useTranslation()
  function update(key: string, value: string) {
    const next = { ...values }
    if (value) next[key] = value
    else delete next[key]
    onChange(next)
  }
  return (
    <div className="form-grid">
      {Object.entries(agent?.parameters ?? {})
        .filter(([key]) => key !== 'model')
        .map(([key, spec]) => {
          const choices = options[key] ?? spec.enum ?? []
          return (
            <Field
              key={key}
              fieldKey={'parameters.' + key}
              label={parameterLabel(spec.label || key)}
              hint={spec.default ? tr('默认：{{v0}}', { v0: spec.default }) : undefined}
            >
              {choices.length ? (
                <ChoicePicker
                  label={parameterLabel(spec.label || key)}
                  value={values[key] ?? ''}
                  disabled={disabled}
                  onChange={(value) => update(key, value)}
                  options={[
                    { value: '', label: tr('使用默认配置') },
                    ...(values[key] && !choices.includes(values[key])
                      ? [{ value: values[key], label: values[key], status: tr('当前值') }]
                      : []),
                    ...choices.map((value) => ({ value, label: value })),
                  ]}
                />
              ) : (
                <input
                  value={values[key] ?? ''}
                  placeholder={spec.default ?? tr('使用默认配置')}
                  onChange={(e) => update(key, e.target.value)}
                />
              )}
            </Field>
          )
        })}
    </div>
  )
}
