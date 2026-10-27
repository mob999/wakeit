import { useEffect, useId, useRef, useState } from 'react'
import * as Menu from '@radix-ui/react-dropdown-menu'
import { t } from '../i18n'
import { IconButton, Button, TooltipTarget } from '../ui'
import { ActionIcon } from '../actions/ActionIcon'
import { DirectoryPicker } from '../DirectoryPicker'
import { ChoicePicker } from '../ChoicePicker'
import type { Runner } from '../api'
import type { InputSchema, Json, Overrides, TemplateIssue } from './types'
import { emptyValue, types } from './document'

const owns = (value: object, key: string) => Object.prototype.hasOwnProperty.call(value, key)
function fieldHint(schema: InputSchema) {
  return [
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
}
export function InputEditor({
  schema,
  value,
  onChange,
  runners,
  runner,
  issues = [],
}: {
  schema: InputSchema
  value: Overrides
  onChange: (value: Overrides) => void
  runners: Runner[]
  runner?: Runner
  issues?: TemplateIssue[]
}) {
  return (
    <div className="template-inputs">
      {Object.entries(schema.properties ?? {})
        .sort(
          ([a], [b]) =>
            Number(!schema.required?.includes(a)) - Number(!schema.required?.includes(b)),
        )
        .map(([name, spec]) => {
          const provided = owns(value.values, name),
            unset = value.unset.includes(name),
            hasDefault = owns(spec, 'default')
          const current = provided
            ? value.values[name]
            : !unset && hasDefault
              ? spec.default
              : undefined
          const change = (next: Json | undefined, omit = false) => {
            const values = { ...value.values }
            delete values[name]
            if (next !== undefined)
              Object.defineProperty(values, name, {
                value: next,
                enumerable: true,
                writable: true,
                configurable: true,
              })
            onChange({
              values,
              unset: [...value.unset.filter((key) => key !== name), ...(omit ? [name] : [])],
            })
          }
          return (
            <div className="template-input-row" key={name}>
              <ValueField
                schema={spec}
                value={current}
                path={`inputs.${name}`}
                required={schema.required?.includes(name)}
                runners={runners}
                runner={runner}
                issues={issues}
                badge={
                  unset
                    ? t('creation.omitted')
                    : !provided && hasDefault
                      ? t('creation.default')
                      : undefined
                }
                onChange={change}
                actions={
                  <>
                    <Menu.Item disabled={!provided && !unset} onSelect={() => change(undefined)}>
                      {t(hasDefault ? 'creation.resetDefault' : 'creation.clearValue')}
                    </Menu.Item>
                    {!schema.required?.includes(name) && hasDefault && (
                      <Menu.Item onSelect={() => change(undefined, true)}>
                        {t('creation.omit')}
                      </Menu.Item>
                    )}
                  </>
                }
              />
            </div>
          )
        })}
      {!Object.keys(schema.properties ?? {}).length && (
        <p className="quiet padded">{t('preset.noInputs')}</p>
      )}
    </div>
  )
}

type ValueProps = {
  schema: InputSchema
  value: Json | undefined
  onChange: (value: Json) => void
  path: string
  required?: boolean
  runner?: Runner
  runners: Runner[]
  issues: TemplateIssue[]
  badge?: string
  actions?: React.ReactNode
  compact?: boolean
  hideLabel?: boolean
}
function ValueField({
  schema,
  value,
  onChange,
  path,
  required,
  runner,
  runners,
  issues,
  badge,
  actions,
  compact,
  hideLabel,
}: ValueProps) {
  const id = useId()
  const kind = types(schema).find((type) => type !== 'null')
  const label = schema.title || path.replace(/^inputs\./, '')
  const hint = [fieldHint(schema), kind === 'array' && schema.items ? fieldHint(schema.items) : '']
    .filter(Boolean)
    .join('\n')
  const controlProps = {
    id,
    'aria-required': required,
    'aria-describedby': hint ? `${id}-hint` : undefined,
    'aria-label': label,
  }
  let control: React.ReactNode
  if (value === null)
    control = (
      <div className="template-null">
        <span>{t('creation.nullValue')}</span>
        {kind && (
          <Button
            onClick={() => {
              const nextSchema = {
                ...schema,
                type: types(schema).filter((type) => type !== 'null'),
                enum: schema.enum?.filter((item) => item !== null),
              }
              delete nextSchema.default
              onChange(emptyValue(nextSchema))
            }}
          >
            {t('taskTemplate.setValue')}
          </Button>
        )}
      </div>
    )
  else if (schema.enum?.length)
    control = (
      <select
        {...controlProps}
        value={value === undefined ? '' : JSON.stringify(value)}
        onChange={(event) => onChange(JSON.parse(event.target.value))}
      >
        {value === undefined && (
          <option value="" disabled>
            {t('请选择')}
          </option>
        )}
        {schema.enum.map((item, index) => (
          <option key={index} value={JSON.stringify(item)}>
            {typeof item === 'string' ? item : JSON.stringify(item)}
          </option>
        ))}
      </select>
    )
  else if (kind === 'object')
    control = (
      <div className="template-object">
        {Object.entries(schema.properties ?? {}).map(([name, spec]) => {
          const object = value && typeof value === 'object' && !Array.isArray(value) ? value : {}
          return (
            <ValueField
              key={name}
              schema={{ ...spec, title: spec.title || name }}
              value={object[name]}
              path={`${path}.${name}`}
              required={schema.required?.includes(name)}
              runner={runner}
              runners={runners}
              issues={issues}
              onChange={(next) => onChange({ ...object, [name]: next })}
              actions={
                !schema.required?.includes(name) && owns(object, name) ? (
                  <Menu.Item
                    onSelect={() => {
                      const next = { ...object }
                      delete next[name]
                      onChange(next)
                    }}
                  >
                    {t('creation.clearValue')}
                  </Menu.Item>
                ) : undefined
              }
            />
          )
        })}
      </div>
    )
  else if (kind === 'array')
    control = (
      <ArrayField
        schema={schema}
        value={Array.isArray(value) ? value : []}
        path={path}
        onChange={onChange}
        runner={runner}
        runners={runners}
        issues={issues}
      />
    )
  else if (kind === 'boolean')
    control = (
      <div className="template-boolean">
        <input
          {...controlProps}
          type="checkbox"
          role="switch"
          checked={value === true}
          onChange={(event) => onChange(event.target.checked)}
        />
        <span className="quiet">
          {t(value === undefined ? 'creation.notSet' : value ? 'creation.yes' : 'creation.no')}
        </span>
      </div>
    )
  else if (kind === 'number' || kind === 'integer')
    control = <NumberField {...controlProps} schema={schema} value={value} onChange={onChange} />
  else if (schema.format === 'directory')
    control = (
      <DirectoryField
        {...controlProps}
        value={typeof value === 'string' ? value : ''}
        onChange={onChange}
        runner={runner}
        runners={runners}
      />
    )
  else if (schema.format === 'multiline')
    control = (
      <textarea
        {...controlProps}
        rows={4}
        value={typeof value === 'string' ? value : ''}
        onChange={(event) => onChange(event.target.value)}
      />
    )
  else
    control = (
      <input
        {...controlProps}
        type={schema.format === 'repository-url' ? 'url' : 'text'}
        value={typeof value === 'string' ? value : value === undefined ? '' : JSON.stringify(value)}
        onChange={(event) => onChange(event.target.value)}
      />
    )
  const valueActions =
    actions ||
    types(schema).includes('null') ||
    (value === undefined && ['array', 'object', 'boolean'].includes(kind ?? ''))
  const menu = valueActions ? (
    <Menu.Root>
      <Menu.Trigger asChild>
        <IconButton
          label={t('creation.parameterActions', {
            name: schema.title || path.replace(/^inputs\./, ''),
          })}
        >
          <ActionIcon name="more" />
        </IconButton>
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Content className="dropdown" align="end">
          {badge && <Menu.Label className="template-value-state">{badge}</Menu.Label>}
          {actions}
          {types(schema).includes('null') && (
            <Menu.Item disabled={value === null} onSelect={() => onChange(null)}>
              {t('creation.setNull')}
            </Menu.Item>
          )}
          {value === undefined && kind === 'boolean' && (
            <>
              <Menu.Item onSelect={() => onChange(true)}>{t('creation.setYes')}</Menu.Item>
              <Menu.Item onSelect={() => onChange(false)}>{t('creation.setNo')}</Menu.Item>
            </>
          )}
          {value === undefined && kind === 'array' && (
            <Menu.Item onSelect={() => onChange([])}>{t('creation.emptyList')}</Menu.Item>
          )}
          {value === undefined && kind === 'object' && (
            <Menu.Item onSelect={() => onChange({})}>{t('creation.emptyObject')}</Menu.Item>
          )}
        </Menu.Content>
      </Menu.Portal>
    </Menu.Root>
  ) : null
  return (
    <section
      className={['template-value-field', compact && 'is-compact', hideLabel && 'is-unlabeled']
        .filter(Boolean)
        .join(' ')}
      data-input-path={path}
    >
      <div className="template-input-heading">
        <label htmlFor={id}>
          {label}
          {required && <span aria-label={t('taskTemplate.required')}> *</span>}
        </label>
        {hint && !hideLabel && (
          <IconButton
            className="template-field-info"
            label={`${label} — ${t('taskTemplate.details')}`}
            tooltip={hint}
          >
            <ActionIcon name="info" />
          </IconButton>
        )}
      </div>
      <div className="template-field-control">
        {control}
        {menu}
      </div>
      {hint && (
        <span id={`${id}-hint`} className="sr-only">
          {hint}
        </span>
      )}
    </section>
  )
}
function NumberField({
  schema,
  value,
  onChange,
  ...props
}: { schema: InputSchema; value: Json | undefined; onChange: (value: Json) => void } & Omit<
  React.InputHTMLAttributes<HTMLInputElement>,
  'value' | 'onChange'
>) {
  const [draft, setDraft] = useState(value === undefined ? '' : String(value))
  const last = useRef(value)
  useEffect(() => {
    if (last.current !== value) {
      setDraft(value === undefined ? '' : String(value))
      last.current = value
    }
  }, [value])
  return (
    <input
      {...props}
      inputMode={types(schema).includes('integer') ? 'numeric' : 'decimal'}
      value={draft}
      onChange={(event) => {
        const text = event.target.value
        setDraft(text)
        // Keep intermediate numeric input intact while invalidating a stale preview.
        const parsed =
          text.trim() !== '' &&
          Number.isFinite(Number(text)) &&
          (!Number.isInteger(Number(text)) || Number.isSafeInteger(Number(text)))
            ? Number(text)
            : text
        last.current = parsed
        onChange(parsed)
      }}
    />
  )
}
function DirectoryField({
  value,
  onChange,
  runner,
  runners,
  ...props
}: Omit<React.InputHTMLAttributes<HTMLInputElement>, 'value' | 'onChange'> & {
  value: string
  onChange: (value: string) => void
  runner?: Runner
  runners: Runner[]
}) {
  const [selected, setSelected] = useState('')
  const browserRunner =
    runner ?? (runners.length === 1 ? runners[0] : runners.find((item) => item.id === selected))
  return (
    <div className="template-directory-field">
      {!runner && runners.length > 1 && (
        <TooltipTarget label={t('creation.directoryDevice')}>
          <div className="template-directory-device">
            <ChoicePicker
              label={t('设备')}
              value={selected}
              placeholder={t('设备')}
              searchable
              onChange={setSelected}
              options={runners.map((item) => ({
                value: item.id,
                label: item.name || item.id,
                status: item.online ? undefined : t('离线'),
              }))}
            />
          </div>
        </TooltipTarget>
      )}
      <div className="template-directory-path">
        <input {...props} value={value} onChange={(event) => onChange(event.target.value)} />
        <DirectoryPicker compact runner={browserRunner} value={value} onChange={onChange} />
      </div>
    </div>
  )
}
function ArrayField({
  schema,
  value,
  onChange,
  path,
  runner,
  runners,
  issues,
}: {
  schema: InputSchema
  value: Json[]
  onChange: (value: Json[]) => void
  path: string
  runner?: Runner
  runners: Runner[]
  issues: TemplateIssue[]
}) {
  const rows = useRef({ value, ids: value.map(() => crypto.randomUUID()) })
  if (rows.current.value !== value)
    rows.current = { value, ids: value.map(() => crypto.randomUUID()) }
  const dragging = useRef<number | null>(null)
  const [draggedId, setDraggedId] = useState<string | null>(null)
  const itemSchema = schema.items ?? { type: 'string' }
  const properties =
    types(itemSchema).includes('object') &&
    !types(itemSchema).includes('null') &&
    !itemSchema.enum?.length
      ? Object.entries(itemSchema.properties ?? {})
      : []
  const columns = properties
    .filter(([, spec]) => !types(spec).some((type) => type === 'object' || type === 'array'))
    .sort(
      ([a, x], [b, y]) =>
        Number(types(x).includes('boolean')) - Number(types(y).includes('boolean')) ||
        Number(!itemSchema.required?.includes(a)) - Number(!itemSchema.required?.includes(b)),
    )
    .slice(0, 3)
  const extra = columns.length
    ? properties.filter(([name]) => !columns.some(([column]) => column === name))
    : []
  const change = (next: Json[], ids = rows.current.ids) => {
    rows.current = { value: next, ids }
    onChange(next)
  }
  const update = (index: number, next: Json) =>
    change(value.map((item, i) => (i === index ? next : item)))
  const move = (from: number, to: number) => {
    if (to < 0 || to >= value.length || from === to) return
    const next = [...value],
      ids = [...rows.current.ids]
    next.splice(to, 0, next.splice(from, 1)[0])
    ids.splice(to, 0, ids.splice(from, 1)[0])
    change(next, ids)
  }
  return (
    <div
      className={`template-array ${columns.length ? 'is-table' : ''}`}
      data-columns={columns.length}
      data-last-boolean={
        (columns.length === 3 && types(columns[2][1]).includes('boolean')) || undefined
      }
      style={
        columns.length
          ? ({
              '--template-columns': columns
                .map(([, spec]) =>
                  types(spec).includes('boolean') ? 'minmax(76px, 0.6fr)' : 'minmax(100px, 1fr)',
                )
                .join(' '),
            } as React.CSSProperties)
          : undefined
      }
    >
      {columns.length > 0 && (
        <div className="template-array-columns">
          <span aria-hidden="true" />
          {columns.map(([name, spec]) => (
            <TooltipTarget key={name} label={fieldHint(spec)} enabled={!!fieldHint(spec)}>
              <span tabIndex={fieldHint(spec) ? 0 : undefined}>
                {spec.title || name}
                {itemSchema.required?.includes(name) ? ' *' : ''}
              </span>
            </TooltipTarget>
          ))}
          <span />
        </div>
      )}
      {value.map((item, index) => {
        const object = item && typeof item === 'object' && !Array.isArray(item) ? item : {}
        const clearProperty = (name: string) =>
          !itemSchema.required?.includes(name) && owns(object, name) ? (
            <Menu.Item
              onSelect={() => {
                const next = { ...object }
                delete next[name]
                update(index, next)
              }}
            >
              {t('creation.clearValue')}
            </Menu.Item>
          ) : undefined
        return (
          <div
            className="template-array-row"
            key={rows.current.ids[index]}
            data-dragging={draggedId === rows.current.ids[index] || undefined}
            data-input-path={`${path}[${index}]`}
            onDragOver={(event) => {
              if (dragging.current === null) return
              event.preventDefault()
              event.dataTransfer.dropEffect = 'move'
            }}
            onDrop={(event) => {
              event.preventDefault()
              if (dragging.current !== null) move(dragging.current, index)
              dragging.current = null
              setDraggedId(null)
            }}
          >
            <div className="template-array-main">
              <IconButton
                label={t('creation.reorderItem')}
                className="template-array-grip"
                draggable
                aria-keyshortcuts="ArrowUp ArrowDown"
                onKeyDown={(event) => {
                  if (!['ArrowUp', 'ArrowDown'].includes(event.key)) return
                  event.preventDefault()
                  move(index, index + (event.key === 'ArrowUp' ? -1 : 1))
                }}
                onDragStart={(event) => {
                  dragging.current = index
                  setDraggedId(rows.current.ids[index])
                  event.dataTransfer.effectAllowed = 'move'
                  event.dataTransfer.setData('text/plain', rows.current.ids[index])
                }}
                onDragEnd={() => {
                  dragging.current = null
                  setDraggedId(null)
                }}
              >
                <span className="template-array-grip-dots" />
              </IconButton>
              {columns.length ? (
                columns.map(([name, spec]) => (
                  <ValueField
                    key={name}
                    compact
                    schema={{ ...spec, title: spec.title || name }}
                    value={object[name]}
                    path={`${path}[${index}].${name}`}
                    required={itemSchema.required?.includes(name)}
                    onChange={(next) => update(index, { ...object, [name]: next })}
                    runner={runner}
                    runners={runners}
                    issues={issues}
                    actions={clearProperty(name)}
                  />
                ))
              ) : (
                <ValueField
                  compact
                  hideLabel
                  schema={{ ...itemSchema, title: itemSchema.title || schema.title || path }}
                  value={item}
                  path={`${path}[${index}]`}
                  onChange={(next) => update(index, next)}
                  runner={runner}
                  runners={runners}
                  issues={issues}
                />
              )}
              <Menu.Root>
                <Menu.Trigger asChild>
                  <IconButton
                    className="template-array-actions"
                    label={t('creation.itemActions', { count: index + 1 })}
                  >
                    <ActionIcon name="more" />
                  </IconButton>
                </Menu.Trigger>
                <Menu.Portal>
                  <Menu.Content className="dropdown" align="end">
                    <Menu.Item disabled={index === 0} onSelect={() => move(index, index - 1)}>
                      {t('taskTemplate.moveUp')}
                    </Menu.Item>
                    <Menu.Item
                      disabled={index === value.length - 1}
                      onSelect={() => move(index, index + 1)}
                    >
                      {t('taskTemplate.moveDown')}
                    </Menu.Item>
                    <Menu.Item
                      onSelect={() =>
                        change(
                          [
                            ...value.slice(0, index + 1),
                            structuredClone(item),
                            ...value.slice(index + 1),
                          ],
                          [
                            ...rows.current.ids.slice(0, index + 1),
                            crypto.randomUUID(),
                            ...rows.current.ids.slice(index + 1),
                          ],
                        )
                      }
                      disabled={schema.maxItems != null && value.length >= schema.maxItems}
                    >
                      {t('creation.duplicateItem')}
                    </Menu.Item>
                    <Menu.Item
                      disabled={value.length <= (schema.minItems ?? 0)}
                      onSelect={() =>
                        change(
                          value.filter((_, i) => i !== index),
                          rows.current.ids.filter((_, i) => i !== index),
                        )
                      }
                    >
                      {t('删除')}
                    </Menu.Item>
                  </Menu.Content>
                </Menu.Portal>
              </Menu.Root>
            </div>
            {extra.length > 0 && (
              <details>
                <summary>{t('creation.moreFields')}</summary>
                {extra.map(([name, spec]) => (
                  <ValueField
                    key={name}
                    schema={{ ...spec, title: spec.title || name }}
                    value={object[name]}
                    path={`${path}[${index}].${name}`}
                    required={itemSchema.required?.includes(name)}
                    onChange={(next) => update(index, { ...object, [name]: next })}
                    runner={runner}
                    runners={runners}
                    issues={issues}
                    actions={clearProperty(name)}
                  />
                ))}
              </details>
            )}
          </div>
        )
      })}
      <div className="template-array-footer">
        <IconButton
          label={t('taskTemplate.addItem')}
          disabled={schema.maxItems != null && value.length >= schema.maxItems}
          onClick={() =>
            change([...value, emptyValue(itemSchema)], [...rows.current.ids, crypto.randomUUID()])
          }
        >
          <ActionIcon name="add" />
        </IconButton>
      </div>
    </div>
  )
}
