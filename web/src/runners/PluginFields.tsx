import { useId } from 'react'
import { t as tr } from '../i18n'
import { ChoicePicker } from '../components/ChoicePicker'
import { ActionIcon } from '../actions/ActionIcon'
import { IconButton, TooltipTarget } from '../components/ui'
export type Schema = {
  type?: string | string[]
  title?: string
  description?: string
  default?: unknown
  properties?: Record<string, Schema>
  required?: string[]
  items?: Schema
  enum?: unknown[]
  minimum?: number
  maximum?: number
  minItems?: number
  maxItems?: number
  oneOf?: Schema[]
  anyOf?: Schema[]
  if?: Schema
  then?: Schema
  else?: Schema
  const?: unknown
  minLength?: number
  maxLength?: number
  format?: string
}
export function PluginFields({
  schema,
  value,
  onChange,
  disabled = false,
  label,
}: {
  schema: Schema
  value: unknown
  onChange: (value: unknown) => void
  disabled?: boolean
  label?: string
}) {
  const id = useId()
  const type = Array.isArray(schema.type) ? schema.type.find((s) => s !== 'null') : schema.type
  const title = label ?? schema.title ?? ''
  if (type === 'object' || schema.properties) {
    const object = (value ?? {}) as Record<string, unknown>
    return (
      <div className="plugin-field-group">
        {Object.entries(schema.properties ?? {}).map(([key, field]) => (
          <PluginFields
            key={key}
            label={`${field.title ?? key}${schema.required?.includes(key) ? ' *' : ''}`}
            schema={field}
            value={object[key] ?? field.default}
            disabled={disabled}
            onChange={(next) => onChange({ ...object, [key]: next })}
          />
        ))}
      </div>
    )
  }
  const description = schema.description && (
    <TooltipTarget label={schema.description}>
      <span className="quiet" tabIndex={0}>
        ⓘ
      </span>
    </TooltipTarget>
  )
  if (type === 'array') {
    const items = Array.isArray(value) ? value : []
    return (
      <div className="plugin-field plugin-array">
        <div className="plugin-field-label">
          <span>{title}</span>
          {description}
          <IconButton
            label={tr('添加')}
            disabled={disabled || (!!schema.maxItems && items.length >= schema.maxItems)}
            onClick={() =>
              onChange([
                ...items,
                schema.items?.default ?? (schema.items?.type === 'object' ? {} : ''),
              ])
            }
          >
            <ActionIcon name="add" />
          </IconButton>
        </div>
        <div className="plugin-array-items">
          {items.map((item, index) => (
            <div className="plugin-array-item" key={index}>
              <PluginFields
                schema={schema.items ?? { type: 'string' }}
                value={item}
                disabled={disabled}
                onChange={(next) => onChange(items.map((v, i) => (i === index ? next : v)))}
              />
              <IconButton
                label={tr('删除')}
                disabled={disabled || items.length <= (schema.minItems ?? 0)}
                onClick={() => onChange(items.filter((_, i) => i !== index))}
              >
                <ActionIcon name="close" />
              </IconButton>
            </div>
          ))}
        </div>
      </div>
    )
  }
  const field = schema.enum ? (
    <ChoicePicker
      label={title}
      value={JSON.stringify(value ?? schema.default ?? '')}
      options={schema.enum.map((v) => ({ value: JSON.stringify(v), label: String(v) }))}
      disabled={disabled}
      onChange={(v) => onChange(JSON.parse(v))}
    />
  ) : type === 'boolean' ? (
    <input
      id={id}
      type="checkbox"
      checked={Boolean(value)}
      disabled={disabled}
      onChange={(e) => onChange(e.target.checked)}
    />
  ) : schema.format === 'multiline' ? (
    <textarea
      id={id}
      value={String(value ?? '')}
      disabled={disabled}
      rows={4}
      onChange={(e) => onChange(e.target.value)}
    />
  ) : (
    <input
      id={id}
      type={type === 'number' || type === 'integer' ? 'number' : 'text'}
      value={String(value ?? '')}
      disabled={disabled}
      min={schema.minimum}
      max={schema.maximum}
      minLength={schema.minLength}
      maxLength={schema.maxLength}
      step={type === 'integer' ? 1 : undefined}
      onChange={(e) =>
        onChange(
          type === 'number' || type === 'integer'
            ? e.target.value === ''
              ? null
              : Number(e.target.value)
            : e.target.value,
        )
      }
    />
  )
  return (
    <div className="plugin-field">
      <div className="plugin-field-label">
        <label htmlFor={id}>{title}</label>
        {description}
      </div>
      <div className="plugin-field-control">{field}</div>
    </div>
  )
}

export function schemaDefaults(schema: Schema): unknown {
  if (schema.default !== undefined) return schema.default
  if (schema.type === 'object' || schema.properties)
    return Object.fromEntries(
      Object.entries(schema.properties ?? {}).flatMap(([key, value]) => {
        const initial = schemaDefaults(value)
        return initial === undefined ? [] : [[key, initial]]
      }),
    )
  return undefined
}
