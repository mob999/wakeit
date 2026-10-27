import { Fragment, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { time, type Runner } from '../api'
import { t, parameterLabel } from '../i18n'
import { Monitor } from '../icons'
import { AgentLogo } from '../AgentLogo'
import { ActionIcon } from '../actions/ActionIcon'
import { TooltipTarget } from '../ui'
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
        <div className="preset-overview-meta quiet">
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
              inputs.map(([name, schema]) => (
                <section className="preset-overview-parameter content-card" key={name}>
                  <div className="preset-overview-parameter-head">
                    <SchemaLabel
                      name={name}
                      schema={schema}
                      required={definition.inputs.required?.includes(name)}
                    />
                    <span className="quiet preset-overview-type">{typeLabel(schema)}</span>
                  </div>
                  <div className="preset-overview-parameter-body">
                    <SchemaValue schema={schema} />
                  </div>
                </section>
              ))
            ) : (
              <p className="quiet preset-overview-empty">{t('preset.noInputs')}</p>
            )}
          </div>
        </section>
        <aside className="task-overview-context" aria-label={t('taskTemplate.bindings')}>
          <h3 className="task-overview-heading">{t('taskTemplate.bindings')}</h3>
          {bindings.length ? (
            bindings.map(([name, binding]) => (
              <section className="preset-overview-environment" key={name}>
                <h4 className="task-overview-heading">{binding.title || name}</h4>
                <Environment defaults={binding.defaults ?? {}} runners={runners} />
              </section>
            ))
          ) : (
            <p className="quiet preset-overview-empty">{t('preset.noBindings')}</p>
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
    schema.minimum != null || schema.maximum != null
      ? t('creation.range', { min: schema.minimum ?? '−∞', max: schema.maximum ?? '∞' })
      : '',
    schema.minItems != null || schema.maxItems != null
      ? t('creation.listRange', { min: schema.minItems ?? 0, max: schema.maxItems ?? '∞' })
      : '',
    schema.minLength != null || schema.maxLength != null
      ? t('creation.textRange', { min: schema.minLength ?? 0, max: schema.maxLength ?? '∞' })
      : '',
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

function SchemaValue({
  schema,
  value,
  nested = false,
}: {
  schema: InputSchema
  value?: Json
  nested?: boolean
}) {
  const current = value === undefined ? schema.default : value
  const kind = (Array.isArray(schema.type) ? schema.type : [schema.type]).find(
    (type) => type !== 'null',
  )
  const properties = Object.entries(schema.properties ?? {})
  let content: ReactNode
  if (kind === 'object' && properties.length) {
    const object = current && typeof current === 'object' && !Array.isArray(current) ? current : {}
    content = (
      <>
        {current === null && <DefaultValue value={null} inline={nested} />}
        <dl className="task-overview-properties preset-overview-fields">
          {properties.map(([name, child]) => (
            <Fragment key={name}>
              <dt>
                <SchemaLabel
                  name={name}
                  schema={child}
                  required={schema.required?.includes(name)}
                />
                <small className="quiet preset-overview-type">{typeLabel(child)}</small>
              </dt>
              <dd>
                <SchemaValue
                  schema={child}
                  value={
                    Object.prototype.hasOwnProperty.call(object, name) ? object[name] : undefined
                  }
                  nested
                />
              </dd>
            </Fragment>
          ))}
        </dl>
      </>
    )
  } else if (kind === 'array' && schema.items) {
    const values = Array.isArray(current) ? current : []
    content = (
      <div className="preset-overview-items">
        {values.length ? (
          values.map((item, index) => (
            <div className="preset-overview-item" key={index}>
              <SchemaValue schema={schema.items!} value={item} nested />
            </div>
          ))
        ) : (
          <>
            <DefaultValue value={current} inline={nested} />
            {(schema.items.properties || schema.items.items) && (
              <div className="preset-overview-item-schema">
                <SchemaValue schema={schema.items} nested />
              </div>
            )}
          </>
        )}
      </div>
    )
  } else {
    content = <DefaultValue value={current} inline={nested} />
  }
  return (
    <>
      {content}
      {!!schema.enum?.length && (
        <dl className="task-overview-properties preset-overview-fields preset-overview-options">
          <dt>{t('可选值')}</dt>
          <dd>
            <span className="preset-overview-value">{schema.enum.map(formatValue).join(', ')}</span>
          </dd>
        </dl>
      )}
    </>
  )
}

function DefaultValue({ value, inline = false }: { value?: Json; inline?: boolean }) {
  const content = (
    <span className={value === undefined ? 'quiet' : 'preset-overview-value'}>
      {value === undefined ? '—' : formatValue(value)}
    </span>
  )
  if (inline) return content
  return (
    <dl className="task-overview-properties preset-overview-fields preset-overview-default">
      <dt>{t('creation.default')}</dt>
      <dd>{content}</dd>
    </dl>
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
          <dd>{model || <span className="quiet">{t('taskOverview.default')}</span>}</dd>
        </>
      )}
      {parameters.map(([name, value]) => (
        <Fragment key={name}>
          <dt>{parameterLabel(agent?.parameters[name]?.label || name)}</dt>
          <dd>{value || <span className="quiet">{t('taskOverview.default')}</span>}</dd>
        </Fragment>
      ))}
    </dl>
  )
}

function typeLabel(schema: InputSchema): string {
  const names: Record<string, string> = {
    string: t('creation.typeText'),
    boolean: t('creation.typeBoolean'),
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
  return typeof value === 'string' ? value || '""' : JSON.stringify(value)
}
