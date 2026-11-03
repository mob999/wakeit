import { useId, useState } from 'react'
import Ajv2020 from 'ajv/dist/2020.js'
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
  allOf?: Schema[]
  $defs?: Record<string, Schema>
  $ref?: string
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
  const [alternative, setAlternative] = useState<number | null>(null)
  if (Array.isArray(schema.type) && schema.type.includes('null')) {
    const variants = schema.type.filter((t) => t !== 'null')
    const child = { ...schema, type: variants.length === 1 ? variants[0] : variants }
    return (
      <div className="plugin-field-group">
        <ChoicePicker
          label={label ?? schema.title ?? tr('plugins.variant')}
          value={value === null ? 'null' : 'value'}
          disabled={disabled}
          options={[
            { value: 'value', label: tr('plugins.hasValue') },
            { value: 'null', label: tr('plugins.nullValue') },
          ]}
          onChange={(option) =>
            onChange(
              option === 'null'
                ? null
                : (schemaDefaults(child) ??
                    (child.type === 'object'
                      ? {}
                      : child.type === 'array'
                        ? []
                        : child.type === 'boolean'
                          ? false
                          : '')),
            )
          }
        />
        {value !== null && (
          <PluginFields
            schema={child}
            value={value}
            onChange={onChange}
            disabled={disabled}
            label={label}
          />
        )}
      </div>
    )
  }
  if (schema.$ref?.startsWith('#/$defs/')) {
    const reference = schema.$defs?.[schema.$ref.slice('#/$defs/'.length)]
    if (reference)
      return (
        <PluginFields
          schema={{ ...reference, $defs: schema.$defs }}
          value={value}
          onChange={onChange}
          disabled={disabled}
          label={label}
        />
      )
  }
  const alternatives = schema.oneOf ?? schema.anyOf
  if (alternatives?.length) {
    const match = alternatives.findIndex((option) =>
      matches({ ...option, $defs: schema.$defs }, value),
    )
    const selected = Math.min(alternative ?? Math.max(0, match), alternatives.length - 1)
    const base = { ...schema, oneOf: undefined, anyOf: undefined }
    return (
      <div className="plugin-field-group">
        <ChoicePicker
          label={label ?? schema.title ?? tr('plugins.variant')}
          value={String(selected)}
          disabled={disabled}
          options={alternatives.map((option, index) => ({
            value: String(index),
            label: option.title ?? tr('plugins.variantNumber', { n: index + 1 }),
          }))}
          onChange={(index) => {
            const next = Number(index)
            setAlternative(next)
            onChange(
              schemaDefaults(alternatives[next]) ??
                (alternatives[next].type === 'object' ? {} : ''),
            )
          }}
        />
        <PluginFields
          schema={mergeSchema(base, alternatives[selected])}
          value={value}
          onChange={onChange}
          disabled={disabled}
          label={label}
        />
      </div>
    )
  }
  if (schema.if) {
    const branch = matches({ ...schema.if, $defs: schema.$defs }, value) ? schema.then : schema.else
    schema = mergeSchema(
      { ...schema, if: undefined, then: undefined, else: undefined },
      branch ?? {},
    )
  }
  if (schema.allOf) {
    schema = schema.allOf.reduce(mergeSchema, { ...schema, allOf: undefined })
  }
  const type = Array.isArray(schema.type) ? schema.type.find((s) => s !== 'null') : schema.type
  const title = label ?? schema.title ?? ''
  if (type === 'object' || schema.properties) {
    const object = (value ?? {}) as Record<string, unknown>
    return (
      <div className="plugin-field-group">
        {title && <div className="plugin-field-label font-semibold">{title}</div>}
        {Object.entries(schema.properties ?? {}).map(([key, field]) => (
          <PluginFields
            key={key}
            label={`${field.title ?? key}${schema.required?.includes(key) ? ' *' : ''}`}
            schema={{ ...field, $defs: schema.$defs }}
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
        <ActionIcon name="info" />
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
  if (schema.const !== undefined) return schema.const
  if (schema.type === 'object' || schema.properties)
    return Object.fromEntries(
      Object.entries(schema.properties ?? {}).flatMap(([key, value]) => {
        const initial =
          schemaDefaults(value) ??
          (schema.required?.includes(key) && value.type === 'boolean' ? false : undefined)
        return initial === undefined ? [] : [[key, initial]]
      }),
    )
  return undefined
}

const formValidator = new Ajv2020({ strict: false, validateFormats: false })
const validationCache = new Map<string, ReturnType<typeof formValidator.compile>>()
function matches(schema: Schema, value: unknown): boolean {
  try {
    const key = JSON.stringify(schema)
    let validate = validationCache.get(key)
    if (!validate) {
      validate = formValidator.compile(schema)
      validationCache.set(key, validate)
    }
    return validate(value) === true
  } catch {
    return false
  }
}
function mergeSchema(base: Schema, next: Schema): Schema {
  return {
    ...base,
    ...next,
    properties:
      base.properties || next.properties ? { ...base.properties, ...next.properties } : undefined,
    required: [...new Set([...(base.required ?? []), ...(next.required ?? [])])],
  }
}
