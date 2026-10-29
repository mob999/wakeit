import { Fragment, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { time, t, parameterLabel } from '../i18n'
import { type Runner } from '../api/types'
import { Monitor } from '../components/icons'
import { AgentLogo } from '../components/AgentLogo'
import { ActionIcon } from '../actions/ActionIcon'
import { Button, TooltipTarget } from '../components/ui'
import type { Binding, InputSchema, Json, TemplateRecord } from './types'
import '../tasks/overview.css'
import './preset-overview.css'

export function PresetOverview({ record, runners }: { record: TemplateRecord; runners: Runner[] }) {
  useTranslation()
  const card = useRef<HTMLElement>(null)
  const [size, setSize] = useState('wide')
  useLayoutEffect(() => {
    const observer = new ResizeObserver(([entry]) => {
      setSize(
        entry.contentRect.width < 480 ? 'small' : entry.contentRect.width < 780 ? 'narrow' : 'wide',
      )
    })
    observer.observe(card.current!)
    return () => observer.disconnect()
  }, [])
  const { definition } = record
  const inputs = Object.entries(definition.inputs.properties ?? {})
  const bindings = Object.entries(definition.bindings)
  return (
    <section
      ref={card}
      className="task-overview preset-overview content-card"
      data-size={size}
      aria-label={t('preset.overview')}
    >
      <header className="card-header preset-overview-summary">
        <h3>{definition.name}</h3>
        <div className="preset-overview-meta quiet text-muted">
          <span>{definition.task.dag ? 'DAG' : t('任务')}</span>
          <span>{t('creation.revision', { count: record.revision })}</span>
          {record.updated_at && (
            <TooltipTarget label={t('更新时间')}>
              <time tabIndex={0} dateTime={record.updated_at}>
                {time(record.updated_at)}
              </time>
            </TooltipTarget>
          )}
          {record.archived_at && <span>{t('已归档')}</span>}
        </div>
      </header>
      {definition.description && (
        <p className="preset-overview-description">{definition.description}</p>
      )}
      <div className="task-overview-grid">
        <section className="task-overview-primary" aria-label={t('taskTemplate.inputs')}>
          <h3 className="task-overview-heading">{t('taskTemplate.inputs')}</h3>
          <div className="preset-overview-sections">
            {inputs.length ? (
              <div
                className="preset-overview-table"
                role="table"
                aria-label={t('taskTemplate.inputs')}
              >
                <div className="preset-overview-row preset-overview-table-head" role="row">
                  <span role="columnheader">{t('preset.parameter')}</span>
                  <span role="columnheader">{t('类型')}</span>
                  <span role="columnheader">{t('creation.default')}</span>
                </div>
                {inputs.map(([name, schema]) => (
                  <SchemaField
                    key={name}
                    name={name}
                    schema={schema}
                    required={definition.inputs.required?.includes(name)}
                  />
                ))}
              </div>
            ) : (
              <p className="quiet text-muted preset-overview-empty">{t('preset.noInputs')}</p>
            )}
          </div>
        </section>
        <aside className="task-overview-context" aria-label={t('taskTemplate.bindings')}>
          <h3 className="task-overview-heading">{t('taskTemplate.bindings')}</h3>
          {bindings.length ? (
            bindings.map(([name, binding]) => (
              <section className="preset-overview-environment" key={name}>
                <h4>{binding.title || name}</h4>
                <Environment defaults={binding.defaults ?? {}} runners={runners} />
              </section>
            ))
          ) : (
            <p className="quiet text-muted preset-overview-empty">{t('preset.noBindings')}</p>
          )}
        </aside>
      </div>
    </section>
  )
}

function SchemaLabel({
  name,
  schema,
  required,
}: {
  name: string
  schema: InputSchema
  required?: boolean
}) {
  const constraints = [
    schema.description,
    schema.format === 'directory'
      ? t('preset.formatDirectory')
      : schema.format === 'repository-url'
        ? t('preset.formatRepository')
        : '',

    schema.minimum != null || schema.maximum != null
      ? t('creation.range', { min: schema.minimum ?? '−∞', max: schema.maximum ?? '∞' })
      : '',
    schema.minItems != null || schema.maxItems != null
      ? t('creation.listRange', { min: schema.minItems ?? 0, max: schema.maxItems ?? '∞' })
      : '',
    schema.minLength != null || schema.maxLength != null
      ? t('creation.textRange', { min: schema.minLength ?? 0, max: schema.maxLength ?? '∞' })
      : '',
    schema.enum?.length ? `${t('可选值')}: ${schema.enum.map(formatValue).join(', ')}` : '',
  ]
    .filter(Boolean)
    .join('\n')
  return (
    <span className="preset-overview-label">
      <span>
        {schema.title || name}
        {required && <span aria-label={t('taskTemplate.required')}> *</span>}
      </span>
      {constraints && (
        <TooltipTarget label={constraints}>
          <button type="button" className="preset-overview-info" aria-label={constraints}>
            <ActionIcon name="info" />
          </button>
        </TooltipTarget>
      )}
    </span>
  )
}

function schemaKind(schema: InputSchema) {
  return (Array.isArray(schema.type) ? schema.type : [schema.type]).find((type) => type !== 'null')
}

function SchemaField({
  name,
  schema,
  required,
  value,
}: {
  name: string
  schema: InputSchema
  required?: boolean
  value?: Json
}) {
  const current = value === undefined ? schema.default : value
  return (
    <div className="preset-overview-field" role="rowgroup">
      <div className="preset-overview-row" role="row">
        <div className="preset-overview-name" role="rowheader">
          <SchemaLabel name={name} schema={schema} required={required} />
        </div>
        <span className="preset-overview-type" role="cell">
          {typeLabel(schema)}
        </span>
        <div className="preset-overview-default" role="cell">
          <SchemaValue schema={schema} value={current} />
        </div>
      </div>
      <SchemaChildren schema={schema} value={current} />
    </div>
  )
}

function SchemaChildren({ schema, value }: { schema: InputSchema; value?: Json }) {
  const kind = schemaKind(schema)
  if (kind === 'object' && Object.keys(schema.properties ?? {}).length) {
    const object = value && typeof value === 'object' && !Array.isArray(value) ? value : {}
    return (
      <div className="preset-overview-item" role="rowgroup">
        {Object.entries(schema.properties!).map(([name, child]) => (
          <SchemaField
            key={name}
            name={name}
            schema={child}
            required={schema.required?.includes(name)}
            value={Object.prototype.hasOwnProperty.call(object, name) ? object[name] : undefined}
          />
        ))}
      </div>
    )
  }
  if (
    kind === 'array' &&
    schema.items &&
    ['object', 'array'].includes(schemaKind(schema.items) ?? '')
  ) {
    const items = Array.isArray(value) && value.length ? value : [undefined]
    return (
      <div className="preset-overview-items">
        {items.map((item, index) =>
          schemaKind(schema.items!) === 'object' &&
          Object.keys(schema.items!.properties ?? {}).length > 0 &&
          item !== null ? (
            <SchemaChildren key={index} schema={schema.items!} value={item} />
          ) : (
            <div className="preset-overview-item" role="rowgroup" key={index}>
              <div className="preset-overview-row" role="row">
                <span role="cell" />
                <span className="preset-overview-type" role="cell">
                  {typeLabel(schema.items!)}
                </span>
                <div className="preset-overview-default" role="cell">
                  <SchemaValue schema={schema.items!} value={item} />
                </div>
              </div>
              <SchemaChildren schema={schema.items!} value={item} />
            </div>
          ),
        )}
      </div>
    )
  }
  return null
}

function SchemaValue({ schema, value }: { schema: InputSchema; value?: Json }) {
  const kind = schemaKind(schema)
  if (value !== null && typeof value === 'object') {
    if (Array.isArray(value)) {
      if (!value.length) return <span className="quiet text-muted">{t('preset.emptyList')}</span>
      if (schema.items && ['object', 'array'].includes(schemaKind(schema.items) ?? '')) return null
      return (
        <div className="preset-overview-list">
          {value.map((item, index) => (
            <DefaultValue key={index} value={item} />
          ))}
        </div>
      )
    }
    if (
      kind === 'object' &&
      Object.keys(schema.properties ?? {}).length &&
      Object.keys(value).length
    )
      return null
    if (!Object.keys(value).length)
      return <span className="quiet text-muted">{t('preset.emptyObject')}</span>
  }
  return (
    <DefaultValue
      value={value}
      multiline={
        schema.format === 'multiline' || (typeof value === 'string' && value.includes('\n'))
      }
    />
  )
}

function DefaultValue({ value, multiline = false }: { value?: Json; multiline?: boolean }) {
  const [expanded, setExpanded] = useState(false)
  const text = value === undefined ? t('creation.notSet') : formatValue(value)
  const empty = value === undefined || value === null || value === ''
  if (
    multiline &&
    typeof value === 'string' &&
    (value.length > 180 || value.split('\n').length > 3)
  )
    return (
      <div>
        <span
          className={`preset-overview-value preset-overview-long-value${expanded ? ' is-expanded' : ''}`}
        >
          {text}
        </span>
        <Button
          className="preset-overview-expand"
          aria-expanded={expanded}
          onClick={() => setExpanded(!expanded)}
        >
          {t(expanded ? '收起' : '展开')}
        </Button>
      </div>
    )
  return (
    <TooltipTarget label={text}>
      <span
        className={`preset-overview-value ${multiline ? 'preset-overview-long-value is-expanded' : 'preset-overview-single-value'}${empty ? ' quiet text-muted' : ''}`}
        tabIndex={0}
      >
        {text}
      </span>
    </TooltipTarget>
  )
}
function Environment({ defaults, runners }: { defaults: Binding; runners: Runner[] }) {
  const runner = runners.find(
    (item) => item.id === defaults.runner || item.name === defaults.runner,
  )
  const agent = runner?.agents.find((item) => item.id === defaults.agent)
  const parameters = Object.entries(defaults.parameters ?? {}).filter(([key]) => key !== 'model')
  const model = defaults.parameters?.model
  const showModel = model !== undefined || agent?.models_supported || !!agent?.parameters.model
  return (
    <dl className="task-overview-properties">
      <dt>{t('设备')}</dt>
      <dd>
        {defaults.runner ? (
          <>
            <Monitor size={14} />
            <span>{runner?.name || defaults.runner}</span>
          </>
        ) : (
          '—'
        )}
      </dd>
      <dt>{t('Agent')}</dt>
      <dd>
        {defaults.agent ? (
          <>
            <AgentLogo name={agent?.name || defaults.agent} logo={agent?.logo} />
            <span>{agent?.name || defaults.agent}</span>
          </>
        ) : (
          '—'
        )}
      </dd>
      {showModel && (
        <>
          <dt>{t('模型')}</dt>
          <dd>{model || <span className="quiet text-muted">{t('taskOverview.default')}</span>}</dd>
        </>
      )}
      {parameters.map(([name, value]) => (
        <Fragment key={name}>
          <dt>{parameterLabel(agent?.parameters[name]?.label || name)}</dt>
          <dd>{value || <span className="quiet text-muted">{t('taskOverview.default')}</span>}</dd>
        </Fragment>
      ))}
    </dl>
  )
}

function typeLabel(schema: InputSchema): string {
  const names: Record<string, string> = {
    string: t('creation.typeText'),
    boolean: t('preset.typeBoolean'),
    integer: t('creation.typeInteger'),
    number: t('creation.typeNumber'),
    array: t('creation.typeList'),
    object: t('creation.typeObject'),
    null: t('creation.nullValue'),
  }
  return (Array.isArray(schema.type) ? schema.type : [schema.type])
    .map((type) =>
      type === 'array' && schema.items
        ? `${names.array} (${typeLabel(schema.items)})`
        : names[type] || type,
    )
    .join(' / ')
}
function formatValue(value: Json): string {
  if (typeof value === 'boolean') return t(value ? 'creation.yes' : 'creation.no')
  if (value === null) return t('creation.nullValue')
  return typeof value === 'string' ? value || t('preset.emptyText') : JSON.stringify(value)
}
