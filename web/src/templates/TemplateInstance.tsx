import YAML from 'yaml'
import { useEffect, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, ApiError, type Runner, type TaskConfig } from '../api'
import { t } from '../i18n'
import { Button, Field } from '../ui'
import { ChoicePicker } from '../ChoicePicker'
import { ContentCard } from '../ContentCard'
import { ErrorToast } from '../Toast'
import { DagGraph } from '../dag/DagGraph'
import { promptText } from '../dag/prompt'
import { outputs } from '../dag/model'
import { InputEditor } from './InputEditor'
import { BindingFields } from './BindingFields'
import { parseOverrides } from './document'
import { definitionFrom, issueText } from './utils'
import type {
  Binding,
  Definition,
  Overrides,
  Preview,
  Request,
  TemplateIssue,
  TemplateRecord,
} from './types'
import './templates.css'

const blank: Overrides = { values: {}, unset: [] }

export type InstanceResult = { task_id: string; run_id: string | null }

export function useTemplateRecords() {
  return useQuery<{ items: TemplateRecord[]; total: number }>({
    queryKey: ['task-templates', 'active'],
    queryFn: () => api('/task-templates?archived=false&page_size=100'),
  })
}

export function TemplatePicker({
  kind,
  value,
  disabled,
  onChange,
}: {
  kind: 'task' | 'dag'
  value: string
  disabled?: boolean
  onChange: (id: string) => void
}) {
  useTranslation()
  const records = useTemplateRecords()
  return (
    <Field label={t('taskTemplate.optional')}>
      <ChoicePicker
        label={t('taskTemplate.optional')}
        value={value}
        disabled={disabled}
        pending={records.isPending}
        searchable
        onChange={onChange}
        options={[
          { value: '', label: t('taskTemplate.none') },
          ...(records.data?.items ?? [])
            .filter((row) => !!row.definition.task.dag === (kind === 'dag'))
            .map((row) => ({
              value: row.id,
              label: row.definition.name,
              keywords: `${row.definition.description ?? ''} ${row.id}`,
              disabled: !!row.archived_at,
            })),
        ]}
      />
    </Field>
  )
}

export function useTemplateInstance({
  record,
  definition: draftDefinition,
  definitionError = '',
  name,
  schedule,
  taskId,
}: {
  record: TemplateRecord | null
  definition?: Definition | null
  definitionError?: string
  name?: string
  schedule?: TaskConfig['schedule']
  taskId?: string
}) {
  const client = useQueryClient()
  const [values, setValues] = useState<Overrides>(blank)
  const [bindings, setBindings] = useState<Record<string, Binding>>({})
  const [valueMode, setValueMode] = useState<'form' | 'json' | 'yaml'>('form')
  const [rawValues, setRawValues] = useState('')
  const [valueError, setValueError] = useState('')
  const [preview, setPreview] = useState<Preview | null>(null)
  const [previewSignature, setPreviewSignature] = useState('')
  const [pendingPreview, setPendingPreview] = useState(false)
  const [failure, setFailure] = useState('')
  const [actionIssues, setActionIssues] = useState<TemplateIssue[]>([])
  const [pending, setPending] = useState(false)
  const [selected, setSelected] = useState('')
  const [generatedTaskId] = useState(() => taskId ?? crypto.randomUUID())
  const generation = useRef(0)
  const operation = useRef<{ signature: string; key: string } | null>(null)
  const definition = draftDefinition ?? record?.definition ?? null
  const request: Request = {
    ...values,
    bindings,
    expected_revision: record?.revision ?? 0,
    task_id: taskId ?? generatedTaskId,
    ...(name ? { name } : {}),
    ...(schedule !== undefined ? { schedule } : {}),
  }
  const signature = JSON.stringify({
    definition,
    request,
    raw: valueError ? rawValues : null,
    definitionError,
    valueError,
  })
  const current = !!definition && !definitionError && !valueError && previewSignature === signature

  useEffect(() => {
    setValues(blank)
    setBindings({})
    setValueMode('form')
    setRawValues('')
    setValueError('')
    setPreview(null)
    setPreviewSignature('')
    setSelected('')
    setFailure('')
    setActionIssues([])
    operation.current = null
  }, [record?.id])

  useEffect(() => {
    const seq = ++generation.current,
      controller = new AbortController()
    setFailure('')
    setActionIssues([])
    if (!definition || definitionError || valueError) {
      setPendingPreview(false)
      return
    }
    setPendingPreview(true)
    const timer = setTimeout(async () => {
      try {
        const result = await api<Preview>(
          draftDefinition
            ? '/task-template-preview'
            : `/task-templates/${encodeURIComponent(record!.id)}/preview`,
          'POST',
          draftDefinition
            ? { definition, request: { ...request, request_id: String(seq) } }
            : { ...request, request_id: String(seq) },
          undefined,
          controller.signal,
        )
        if (seq !== generation.current || controller.signal.aborted) return
        setPreview(result)
        setPreviewSignature(signature)
        setSelected((id) =>
          result.graph?.nodes.some((node) => node.id === id)
            ? id
            : (result.graph?.nodes[0]?.id ?? ''),
        )
      } catch (error) {
        if (!controller.signal.aborted && seq === generation.current) {
          setFailure((error as Error).message)
          if (error instanceof ApiError)
            setActionIssues((error.body as { issues?: TemplateIssue[] })?.issues ?? [])
        }
      } finally {
        if (!controller.signal.aborted && seq === generation.current) setPendingPreview(false)
      }
    }, 300)
    return () => {
      clearTimeout(timer)
      controller.abort()
    }
  }, [signature])

  function raw(mode: 'form' | 'json' | 'yaml') {
    setValueMode(mode)
    if (valueError || mode === 'form') return
    setRawValues(
      mode === 'json' ? JSON.stringify(values, null, 2) : YAML.stringify(values, { lineWidth: 0 }),
    )
  }

  function changeRaw(source: string) {
    setRawValues(source)
    try {
      setValues(parseOverrides(source))
      setValueError('')
    } catch (error) {
      setValueError((error as Error).message)
    }
  }

  async function create(run_now = false): Promise<InstanceResult> {
    if (pendingPreview) throw new Error(t('taskTemplate.updating'))
    if (!record || !current || !preview?.config) throw new Error(t('taskTemplate.stale'))
    const body = { ...request, run_now },
      keySignature = JSON.stringify(body)
    if (operation.current?.signature !== keySignature)
      operation.current = { signature: keySignature, key: crypto.randomUUID() }
    setPending(true)
    setActionIssues([])
    try {
      const result = await api<InstanceResult>(
        `/task-templates/${encodeURIComponent(record.id)}/instantiate`,
        'POST',
        body,
        operation.current.key,
      )
      client.invalidateQueries({ queryKey: ['tasks'] })
      client.invalidateQueries({ queryKey: ['runs'] })
      return result
    } catch (error) {
      if (error instanceof ApiError)
        setActionIssues((error.body as { issues?: TemplateIssue[] })?.issues ?? [])
      throw error
    } finally {
      setPending(false)
    }
  }

  return {
    record,
    definition,
    request,
    preview,
    current,
    pending,
    pendingPreview,
    failure,
    values,
    setValues,
    bindings,
    setBindings,
    valueMode,
    rawValues,
    valueError,
    raw,
    changeRaw,
    selected,
    setSelected,
    issues: [...(preview?.issues ?? []), ...actionIssues],
    dirty:
      Object.keys(values.values).length > 0 ||
      values.unset.length > 0 ||
      Object.keys(bindings).length > 0 ||
      !!valueError,
    create,
  }
}

export function TemplateInstanceView({
  instance,
  showCreationStatus = true,
}: {
  instance: ReturnType<typeof useTemplateInstance>
  showCreationStatus?: boolean
}) {
  useTranslation()
  const runners = useQuery<Runner[]>({ queryKey: ['runners'], queryFn: () => api('/runners') })
  const {
    definition,
    preview,
    current,
    pendingPreview,
    failure,
    values,
    setValues,
    bindings,
    setBindings,
    valueMode,
    rawValues,
    valueError,
    raw,
    changeRaw,
    selected,
    setSelected,
    issues,
  } = instance
  const allRunners = (runners.data ?? []).filter((runner) => !runner.revoked)
  const knownBindings = definition?.bindings ?? {}
  const visibleBindings = preview?.used_bindings.length
    ? Object.keys(knownBindings).filter((key) => preview.used_bindings.includes(key))
    : Object.keys(knownBindings)
  const generated = preview?.graph,
    selectedNode = generated?.nodes.find((node) => node.id === selected)
  return (
    <div className="template-instance">
      <ContentCard
        title={t('taskTemplate.inputs')}
        actions={
          <div
            className="template-value-modes"
            role="group"
            aria-label={t('taskTemplate.overrideMode')}
          >
            {(['form', 'json', 'yaml'] as const).map((mode) => (
              <Button
                key={mode}
                aria-pressed={valueMode === mode}
                disabled={!!valueError && mode === 'form'}
                onClick={() => raw(mode)}
              >
                {mode === 'form' ? t('taskTemplate.form') : mode.toUpperCase()}
              </Button>
            ))}
          </div>
        }
      >
        {valueMode === 'form' && definition ? (
          <InputEditor
            schema={definition.inputs}
            value={values}
            onChange={setValues}
            runners={allRunners}
          />
        ) : (
          <>
            <textarea
              className="mono template-values"
              spellCheck={false}
              aria-label={t('taskTemplate.overrides')}
              value={rawValues}
              onChange={(event) => changeRaw(event.target.value)}
            />
            {valueError && (
              <p className="template-error" role="alert">
                {t(valueError)}
              </p>
            )}
          </>
        )}
        {definition && visibleBindings.length > 0 && (
          <section className="template-bindings">
            <h3>{t('taskTemplate.bindings')}</h3>
            {visibleBindings.map((slot) => (
              <BindingFields
                key={slot}
                title={knownBindings[slot].title || slot}
                slot={slot}
                defaults={knownBindings[slot].defaults ?? {}}
                value={bindings[slot] ?? {}}
                runners={allRunners}
                onChange={(value) => setBindings({ ...bindings, [slot]: value })}
              />
            ))}
          </section>
        )}
      </ContentCard>
      <ContentCard
        title={t('taskTemplate.preview')}
        actions={
          <div className="template-preview-status">
            {generated && (
              <small className="quiet">
                {t('dag.nodeCount', { count: generated.nodes.length })} ·{' '}
                {t('moments.outputs', { count: outputs(generated).length })}
              </small>
            )}
            {showCreationStatus && (
              <small className="quiet" role="status">
                {t(
                  pendingPreview
                    ? 'taskTemplate.updating'
                    : !current
                      ? 'taskTemplate.stale'
                      : 'taskTemplate.current',
                )}
              </small>
            )}
          </div>
        }
      >
        <div className="template-generated" aria-busy={pendingPreview}>
          {failure && <ErrorToast>{failure}</ErrorToast>}
          {generated && (
            <DagGraph
              compact
              hideTitle
              nodes={generated.nodes}
              runners={allRunners}
              selected={selected}
              onSelect={setSelected}
              outputIds={outputs(generated)}
            />
          )}
          {selectedNode && (
            <section className="template-generated-instruction">
              <div className="template-node-heading">
                <strong>{selectedNode.name}</strong>
                <code>{selectedNode.id}</code>
              </div>
              <dl className="properties">
                <dt>{t('设备')}</dt>
                <dd>
                  {allRunners.find(
                    (runner) =>
                      runner.id === selectedNode.runner || runner.name === selectedNode.runner,
                  )?.name ||
                    selectedNode.runner ||
                    '—'}
                </dd>
                <dt>Agent</dt>
                <dd>{selectedNode.agent || '—'}</dd>
                <dt>{t('工作目录')}</dt>
                <dd>{selectedNode.cwd || '—'}</dd>
                {preview?.source_map[selectedNode.id]?.input_path && (
                  <>
                    <dt>{t('taskTemplate.inputSource')}</dt>
                    <dd>{preview.source_map[selectedNode.id].input_path}</dd>
                  </>
                )}
              </dl>
              <pre className="template-prompt">{promptText(selectedNode.prompt)}</pre>
            </section>
          )}
          {!generated && preview?.config && (
            <section className="template-generated-instruction">
              <strong>{preview.config.name}</strong>
              <p className="quiet mono">{preview.config.cwd}</p>
              <pre className="template-prompt">{preview.config.prompt}</pre>
            </section>
          )}
          {!preview && !failure && <p className="quiet padded">{t('taskTemplate.fillInputs')}</p>}
          {issues.length > 0 && <TemplateIssues issues={issues} onSelect={setSelected} />}
        </div>
      </ContentCard>
    </div>
  )
}

function TemplateIssues({
  issues,
  onSelect,
}: {
  issues: TemplateIssue[]
  onSelect: (id: string) => void
}) {
  useTranslation()
  return (
    <ul className="template-issues" aria-label={t('taskTemplate.validation')}>
      {issues.map((issue, index) => (
        <li key={`${issue.path}-${index}`}>
          <button
            type="button"
            onClick={() => {
              if (issue.node_id) onSelect(issue.node_id)
              const name = /^inputs\.([^.\[]+)/.exec(issue.path)?.[1]
              if (name)
                requestAnimationFrame(() =>
                  document
                    .querySelector<HTMLElement>(`[data-input="${CSS.escape(name)}"]`)
                    ?.scrollIntoView({ block: 'nearest' }),
                )
            }}
          >
            <code>
              {issue.path}
              {issue.line ? `:${issue.line}:${issue.column ?? 1}` : ''}
            </code>
            <span>{issueText(issue)}</span>
          </button>
          {['render', 'expression', 'syntax', 'configuration', 'binding'].includes(issue.code) && (
            <details>
              <summary>{t('taskTemplate.details')}</summary>
              <pre>{issue.message}</pre>
            </details>
          )}
        </li>
      ))}
    </ul>
  )
}

export { definitionFrom }
