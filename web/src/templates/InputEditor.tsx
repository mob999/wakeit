import { useRef, useState, type ReactNode } from 'react'
import { t } from '../i18n'
import { IconButton } from '../ui'
import { ActionIcon } from '../actions/ActionIcon'
import { DirectoryPicker } from '../DirectoryPicker'
import type { Runner } from '../api'
import type { InputSchema, Json, Overrides } from './types'
import { emptyValue, types } from './document'
const owns = (value: object, key: string) => Object.prototype.hasOwnProperty.call(value, key)
export function InputEditor({
  schema,
  value,
  onChange,
  runners,
}: {
  schema: InputSchema
  value: Overrides
  onChange: (v: Overrides) => void
  runners: Runner[]
}) {
  const [pickerRunner, setPickerRunner] = useState('')
  const runner = runners.find((r) => r.id === pickerRunner)
  return (
    <div className="template-inputs">
      {Object.entries(schema.properties ?? {}).map(([name, spec]) => {
        const provided = owns(value.values, name),
          unset = value.unset.includes(name),
          hasDefault = owns(spec, 'default')
        const current = provided
          ? value.values[name]
          : !unset && hasDefault
            ? spec.default
            : undefined
        const mode = provided ? (current === null ? 'null' : 'value') : unset ? 'unset' : 'default'
        function update(next: Json | undefined, choice = 'value') {
          const values = { ...value.values }
          delete values[name]
          if (choice === 'value' || choice === 'null')
            Object.defineProperty(values, name, {
              value: next ?? null,
              enumerable: true,
              writable: true,
              configurable: true,
            })
          onChange({
            values,
            unset: [
              ...value.unset.filter((k) => k !== name),
              ...(choice === 'unset' ? [name] : []),
            ],
          })
        }
        return (
          <section key={name} className="template-input-row" data-input={name}>
            <div className="template-input-heading">
              <label>
                {spec.title || name}
                {schema.required?.includes(name) ? ' *' : ''}
              </label>
              <select
                aria-label={t('taskTemplate.valueMode', { name: spec.title || name })}
                value={mode}
                onChange={(e) => {
                  const mode = e.target.value
                  update(mode === 'value' ? (current ?? emptyValue(spec)) : null, mode)
                }}
              >
                <option value="default">
                  {t(hasDefault ? 'taskTemplate.useDefault' : 'taskTemplate.omitted')}
                </option>
                <option value="value">{t('taskTemplate.override')}</option>
                {hasDefault && <option value="unset">{t('taskTemplate.unset')}</option>}
                {types(spec).includes('null') && <option value="null">null</option>}
              </select>
            </div>
            {spec.description && <small className="quiet">{spec.description}</small>}
            {current !== undefined && current !== null && (
              <ValueField
                schema={spec}
                value={current}
                path={`inputs.${name}`}
                onChange={(v) => update(v)}
                runner={runner}
              />
            )}
            {spec.format === 'directory' && (
              <div className="template-directory-device">
                <label>{t('taskTemplate.browseOn')}</label>
                <select
                  aria-label={t('taskTemplate.browseOn')}
                  value={pickerRunner}
                  onChange={(e) => setPickerRunner(e.target.value)}
                >
                  <option value="">{t('先选择 Runner')}</option>
                  {runners.map((r) => (
                    <option key={r.id} value={r.id}>
                      {r.name || r.id}
                    </option>
                  ))}
                </select>
              </div>
            )}
          </section>
        )
      })}
    </div>
  )
}
function ValueField({
  schema,
  value,
  onChange,
  path,
  runner,
}: {
  schema: InputSchema
  value: Json
  onChange: (v: Json) => void
  path: string
  runner?: Runner
}) {
  const kind = types(schema).find((t) => t !== 'null')
  const label = schema.title || path
  if (schema.enum?.length)
    return (
      <select
        aria-label={label}
        value={JSON.stringify(value)}
        onChange={(e) => onChange(JSON.parse(e.target.value))}
      >
        {!schema.enum.some((v) => JSON.stringify(v) === JSON.stringify(value)) && (
          <option value={JSON.stringify(value)}>{JSON.stringify(value)}</option>
        )}
        {schema.enum.map((v, i) => (
          <option key={i} value={JSON.stringify(v)}>
            {typeof v === 'string' ? v : JSON.stringify(v)}
          </option>
        ))}
      </select>
    )
  if (kind === 'array' && Array.isArray(value))
    return (
      <ArrayField schema={schema} value={value} path={path} onChange={onChange} runner={runner} />
    )
  if (kind === 'object' && value && typeof value === 'object' && !Array.isArray(value))
    return (
      <div className="template-object">
        {Object.entries(schema.properties ?? {}).map(([name, spec]) => (
          <label key={name}>
            <span>
              {spec.title || name}
              {schema.required?.includes(name) ? ' *' : ''}
            </span>
            <div>
              {owns(value, name) ? (
                <>
                  <ValueField
                    schema={spec}
                    value={value[name]}
                    path={path + '.' + name}
                    onChange={(v) => onChange({ ...value, [name]: v })}
                    runner={runner}
                  />
                  {!schema.required?.includes(name) && (
                    <button
                      type="button"
                      onClick={() => {
                        const next = { ...value }
                        delete next[name]
                        onChange(next)
                      }}
                    >
                      {t('taskTemplate.omit')}
                    </button>
                  )}
                  {types(spec).includes('null') && value[name] !== null && (
                    <button type="button" onClick={() => onChange({ ...value, [name]: null })}>
                      null
                    </button>
                  )}
                </>
              ) : (
                <button
                  type="button"
                  onClick={() => onChange({ ...value, [name]: emptyValue(spec) })}
                >
                  {t('taskTemplate.setValue')}
                </button>
              )}
            </div>
          </label>
        ))}
      </div>
    )
  if (value === null)
    return (
      <button
        type="button"
        onClick={() =>
          onChange(emptyValue({ ...schema, type: types(schema).filter((t) => t !== 'null') }))
        }
      >
        {t('taskTemplate.setValue')}
      </button>
    )
  if (kind === 'boolean' && typeof value === 'boolean')
    return (
      <select
        aria-label={label}
        value={String(value)}
        onChange={(e) => onChange(e.target.value === 'true')}
      >
        <option value="true">true</option>
        <option value="false">false</option>
      </select>
    )
  if (kind === 'number' || kind === 'integer')
    return (
      <input
        aria-label={label}
        inputMode="decimal"
        value={typeof value === 'object' ? JSON.stringify(value) : String(value)}
        onChange={(e) => {
          const text = e.target.value
          onChange(
            text !== '' &&
              Number.isFinite(Number(text)) &&
              (!Number.isInteger(Number(text)) || Number.isSafeInteger(Number(text)))
              ? Number(text)
              : text,
          )
        }}
      />
    )
  if (typeof value === 'string')
    return (
      <>
        {schema.format === 'multiline' ? (
          <textarea
            aria-label={label}
            rows={5}
            value={value}
            onChange={(e) => onChange(e.target.value)}
          />
        ) : (
          <input aria-label={label} value={value} onChange={(e) => onChange(e.target.value)} />
        )}
        {schema.format === 'directory' && (
          <DirectoryPicker runner={runner} value={value} onChange={onChange} />
        )}
      </>
    )
  return <RawValue value={value} onChange={onChange} label={label} />
}
function RawValue({
  value,
  onChange,
  label,
}: {
  value: Json
  onChange: (v: Json) => void
  label: string
}) {
  const [draft, setDraft] = useState(JSON.stringify(value))
  return (
    <textarea
      aria-label={label}
      value={draft}
      onChange={(e) => {
        setDraft(e.target.value)
        try {
          onChange(JSON.parse(e.target.value))
        } catch {
          onChange(e.target.value)
        }
      }}
    />
  )
}
function ArrayField({
  schema,
  value,
  onChange,
  path,
  runner,
}: {
  schema: InputSchema
  value: Json[]
  onChange: (v: Json) => void
  path: string
  runner?: Runner
}) {
  const rows = useRef<{ value: Json[]; ids: string[] }>({
    value,
    ids: value.map(() => crypto.randomUUID()),
  })
  if (rows.current.value !== value) {
    rows.current = { value, ids: value.map(() => crypto.randomUUID()) }
  }
  const dragging = useRef<number | null>(null)
  function change(next: Json[], ids: string[]) {
    rows.current = { value: next, ids }
    onChange(next)
  }
  function move(from: number, to: number) {
    if (to < 0 || to >= value.length || from === to) return
    const next = [...value],
      ids = [...rows.current.ids]
    next.splice(to, 0, next.splice(from, 1)[0])
    ids.splice(to, 0, ids.splice(from, 1)[0])
    change(next, ids)
  }
  function control(index: number): ReactNode {
    return (
      <div className="template-array-actions">
        <span className="quiet">{index + 1}</span>
        <IconButton
          label={t('taskTemplate.moveUp')}
          disabled={!index}
          onClick={() => move(index, index - 1)}
        >
          <ActionIcon name="up" />
        </IconButton>
        <IconButton
          label={t('taskTemplate.moveDown')}
          disabled={index === value.length - 1}
          onClick={() => move(index, index + 1)}
        >
          <ActionIcon name="latest" />
        </IconButton>
        <IconButton
          label={t('删除')}
          onClick={() =>
            change(
              value.filter((_, i) => i !== index),
              rows.current.ids.filter((_, i) => i !== index),
            )
          }
        >
          <ActionIcon name="delete" />
        </IconButton>
      </div>
    )
  }
  return (
    <div className="template-array">
      {value.map((item, index) => (
        <div
          key={rows.current.ids[index]}
          className="template-array-row"
          data-input-path={path + '[' + index + ']'}
          draggable
          onDragStart={(e) => {
            if ((e.target as HTMLElement).closest('input,textarea,select,button')) {
              e.preventDefault()
              return
            }
            dragging.current = index
            e.dataTransfer.setData('text/plain', rows.current.ids[index])
          }}
          onDragOver={(e) => e.preventDefault()}
          onDrop={(e) => {
            e.preventDefault()
            if (dragging.current !== null) move(dragging.current, index)
            dragging.current = null
          }}
        >
          {control(index)}
          <ValueField
            schema={schema.items!}
            value={item}
            path={path + '[' + index + ']'}
            runner={runner}
            onChange={(v) =>
              change(
                value.map((item, i) => (i === index ? v : item)),
                [...rows.current.ids],
              )
            }
          />
        </div>
      ))}
      <button
        type="button"
        onClick={() =>
          change([...value, emptyValue(schema.items!)], [...rows.current.ids, crypto.randomUUID()])
        }
      >
        {t('taskTemplate.addItem')}
      </button>
    </div>
  )
}
