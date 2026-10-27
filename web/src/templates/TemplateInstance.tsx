import { ScheduleSummary } from '../ScheduleSummary'
import YAML from 'yaml'
import { useEffect, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, ApiError, type Runner, type TaskConfig } from '../api'
import { t } from '../i18n'
import { Button } from '../ui'
import { CopyValue } from '../runs/CopyValue'
import { useToast } from '../Toast'
import { ErrorToast } from '../Toast'
import { DagGraph } from '../dag/DagGraph'
import { promptText } from '../dag/prompt'
import { outputs } from '../dag/model'
import { InputEditor } from './InputEditor'
import { BindingFields } from './BindingFields'
import { parseOverrides } from './document'
import { issueText } from './utils'
import type { Binding, Overrides, Preview, Request, TemplateIssue, TemplateRecord } from './types'
import './templates.css'

const blank: Overrides = { values: {}, unset: [] }

export type InstanceResult = { task_id: string; run_id: string | null }

export function useTemplateInstance({
  record,
  active = true,
}: {
  record: TemplateRecord
  active?: boolean
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
  const [refresh, setRefresh] = useState(0)
  const [selected, setSelected] = useState('')
  const [generatedTaskId] = useState(() => crypto.randomUUID())
  const generation = useRef(0)
  const operation = useRef<{ signature: string; key: string } | null>(null)
  const definition = record.definition
  const request: Request = {
    ...values,
    bindings,
    expected_revision: record.revision,
    task_id: generatedTaskId,
  }
  const signature = JSON.stringify({
    definition,
    request,
    raw: valueError ? rawValues : null,
    valueError,
  })
  const fresh = !!definition && !valueError && previewSignature === signature
  const current = fresh && !!preview?.config && !preview.issues.length && !record?.archived_at

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

  const revision = useRef(record.revision)
  useEffect(() => {
    if (revision.current === record.revision) return
    revision.current = record.revision
    const properties = record.definition.inputs.properties ?? {}
    setValues((previous) => ({
      values: Object.fromEntries(
        Object.entries(previous.values).filter(([key]) => key in properties),
      ),
      unset: previous.unset.filter((key) => key in properties),
    }))
    setBindings((previous) =>
      Object.fromEntries(
        Object.entries(previous).filter(([slot]) => slot in record.definition.bindings),
      ),
    )
    setValueMode('form')
    setValueError('')
  }, [record.revision])

  useEffect(() => {
    const seq = ++generation.current,
      controller = new AbortController()
    setFailure('')
    setActionIssues([])
    if (!active || valueError) {
      setPendingPreview(false)
      return
    }
    setPendingPreview(true)
    setPreviewSignature('')
    const timer = setTimeout(async () => {
      try {
        const result = await api<Preview>(
          `/task-templates/${encodeURIComponent(record.id)}/preview`,
          'POST',
          { ...request, request_id: String(seq) },
          undefined,
          controller.signal,
        )
        if (seq !== generation.current || controller.signal.aborted) return
        setPreview(result)
        setPreviewSignature(signature)
        if (result.graph)
          setSelected((id) =>
            result.graph!.nodes.some((node) => node.id === id)
              ? id
              : (result.graph!.nodes[0]?.id ?? ''),
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
  }, [signature, active, refresh])

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
    fresh,
    pending,
    pendingPreview,
    failure,
    refreshPreview: () => setRefresh((value) => value + 1),
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
    issues: [...(fresh ? (preview?.issues ?? []) : []), ...actionIssues],
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
  issueRequest = 0,
}: {
  instance: ReturnType<typeof useTemplateInstance>
  issueRequest?: number
}) {
  useTranslation()
  const runners = useQuery<Runner[]>({ queryKey: ['runners'], queryFn: () => api('/runners') })
  const {
    definition,
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
  const [pane, setPane] = useState<'parameters' | 'preview'>('parameters')
  const root = useRef<HTMLDivElement>(null)
  const lastPreview = useRef<Preview | null>(null)
  if (instance.preview?.config) lastPreview.current = instance.preview
  const preview = lastPreview.current
  const allRunners = (runners.data ?? []).filter((runner) => !runner.revoked)
  const knownBindings = definition?.bindings ?? {}
  const visibleBindings =
    instance.fresh &&
    instance.preview &&
    (instance.preview.config || instance.preview.used_bindings.length)
      ? Object.keys(knownBindings).filter((key) => instance.preview!.used_bindings.includes(key))
      : Object.keys(knownBindings)
  const boundRunners = [
    ...new Set(
      visibleBindings
        .map((key) => bindings[key]?.runner ?? knownBindings[key].defaults?.runner)
        .filter(Boolean),
    ),
  ]
    .map((id) => allRunners.find((runner) => runner.id === id || runner.name === id))
    .filter((runner): runner is Runner => !!runner)
  const browsingRunner = boundRunners.length === 1 ? boundRunners[0] : undefined
  const generated = preview?.graph,
    selectedNode = generated?.nodes.find((node) => node.id === selected)
  const focusIssue = (issue: TemplateIssue) => {
    if (issue.node_id && !/^(inputs|bindings)(\.|$)/.test(issue.path)) {
      setSelected(issue.node_id)
      setPane('preview')
    } else {
      setPane('parameters')
      if (valueMode !== 'form' && !valueError) raw('form')
    }
    requestAnimationFrame(() => {
      const fields = [...(root.current?.querySelectorAll<HTMLElement>('[data-input-path]') ?? [])]
      const directoryFields = [
        ...(root.current?.querySelectorAll<HTMLInputElement>('.template-directory-path > input') ??
          []),
      ]
      const nodeDirectory =
        issue.params?.directory ??
        instance.preview?.graph?.nodes.find((node) => node.id === issue.node_id)?.cwd ??
        generated?.nodes.find((node) => node.id === issue.node_id)?.cwd
      const directory =
        issue.path.startsWith('bindings.') && issue.path.endsWith('.cwd')
          ? (directoryFields.find((input) => input.value === nodeDirectory) ??
            (directoryFields.length === 1 ? directoryFields[0] : undefined))
          : undefined
      const field =
        directory?.closest<HTMLElement>('[data-input-path]') ??
        fields.find((item) => item.dataset.inputPath === issue.path) ??
        fields
          .filter(
            (item) =>
              issue.path.startsWith(item.dataset.inputPath + '.') ||
              issue.path.startsWith(item.dataset.inputPath + '['),
          )
          .sort(
            (a, b) => (b.dataset.inputPath?.length ?? 0) - (a.dataset.inputPath?.length ?? 0),
          )[0]
      if (field) {
        if (field instanceof HTMLDetailsElement) field.open = true
        let parent = field.parentElement
        while (parent && parent !== root.current) {
          if (parent instanceof HTMLDetailsElement) parent.open = true
          parent = parent.parentElement
        }
        const target =
          field.querySelector<HTMLElement>('input,textarea,select,[role=combobox]') ??
          field.querySelector<HTMLElement>('button')
        field.scrollIntoView({ block: 'nearest' })
        target?.focus({ preventScroll: true })
      } else {
        const summary = root.current?.querySelector<HTMLElement>('.template-issues')
        summary?.scrollIntoView({ block: 'nearest' })
        summary?.querySelector<HTMLButtonElement>('button')?.focus({ preventScroll: true })
      }
    })
  }
  useEffect(() => {
    if (issueRequest && issues[0]) focusIssue(issues[0])
  }, [issueRequest])
  return (
    <div className="template-instance" ref={root} data-pane={pane}>
      <div className="template-mobile-switch" role="group" aria-label={t('creation.presetView')}>
        <Button aria-pressed={pane === 'parameters'} onClick={() => setPane('parameters')}>
          {t('taskTemplate.inputs')}
        </Button>
        <Button aria-pressed={pane === 'preview'} onClick={() => setPane('preview')}>
          {t('creation.taskPreview')}
        </Button>
      </div>
      <section className="template-parameters-pane" aria-label={t('taskTemplate.inputs')}>
        <header className="template-pane-heading">
          <strong>{t('taskTemplate.inputs')}</strong>
          <details className="template-raw-menu">
            <summary>{t('高级设置')}</summary>
            <div
              className="template-value-modes"
              role="group"
              aria-label={t('taskTemplate.overrideMode')}
            >
              {(['form', 'json', 'yaml'] as const).map((mode) => (
                <Button
                  key={mode}
                  aria-pressed={valueMode === mode}
                  disabled={!!valueError && mode !== valueMode}
                  onClick={() => raw(mode)}
                >
                  {mode === 'form' ? t('taskTemplate.form') : mode.toUpperCase()}
                </Button>
              ))}
            </div>
          </details>
        </header>
        <div className="template-pane-scroll">
          {definition && visibleBindings.length > 0 && (
            <section className="template-bindings" aria-label={t('taskTemplate.bindings')}>
              {runners.isPending ? (
                <p className="quiet padded" role="status">
                  {t('正在加载…')}
                </p>
              ) : runners.error ? (
                <ErrorToast>
                  {runners.error.message}
                  <Button onClick={() => runners.refetch()}>{t('重试')}</Button>
                </ErrorToast>
              ) : !allRunners.length ? (
                <p className="quiet padded">{t('暂无设备')}</p>
              ) : (
                visibleBindings.map((slot) => {
                  const context = instance.fresh ? instance.preview : preview
                  const graph = context?.graph
                  const nodes =
                    graph?.nodes.filter((node) => context?.source_map[node.id]?.binding === slot) ??
                    []
                  const directories = [...new Set(nodes.map((node) => node.cwd).filter(Boolean))]
                  const chosen = bindings[slot] ?? {},
                    defaults = knownBindings[slot].defaults ?? {}
                  const effectiveRunner = chosen.runner ?? defaults.runner,
                    effectiveAgent = chosen.agent ?? defaults.agent
                  const sameRunner = (id: string) => {
                    const runner = allRunners.find(
                      (runner) => runner.id === id || runner.name === id,
                    )
                    return (
                      effectiveRunner === id ||
                      runner?.id === effectiveRunner ||
                      runner?.name === effectiveRunner
                    )
                  }
                  const contextMatches = graph
                    ? nodes.every(
                        (node) => sameRunner(node.runner) && node.agent === effectiveAgent,
                      )
                    : preview?.config
                      ? sameRunner(preview.config.runner) && preview.config.agent === effectiveAgent
                      : false
                  const contexts = !contextMatches
                    ? []
                    : graph
                      ? directories
                      : preview?.config?.cwd
                        ? [preview.config.cwd]
                        : []
                  return (
                    <BindingFields
                      key={slot}
                      title={knownBindings[slot].title || slot}
                      slot={slot}
                      defaults={knownBindings[slot].defaults ?? {}}
                      value={bindings[slot] ?? {}}
                      runners={allRunners}
                      directories={contexts}
                      issues={issues}
                      onChange={(value) => setBindings({ ...bindings, [slot]: value })}
                    />
                  )
                })
              )}
            </section>
          )}
          {valueMode === 'form' && definition ? (
            <InputEditor
              schema={definition.inputs}
              value={values}
              onChange={setValues}
              runners={allRunners}
              runner={browsingRunner}
              issues={issues}
            />
          ) : (
            <div className="template-raw-values">
              <textarea
                className="mono template-values"
                spellCheck={false}
                aria-label={t('taskTemplate.overrides')}
                value={rawValues}
                onChange={(event) => changeRaw(event.target.value)}
              />
              {valueError && (
                <p className="template-field-error" role="alert">
                  {t(valueError)}
                </p>
              )}
            </div>
          )}
        </div>
      </section>
      <section
        className="template-preview-pane"
        aria-label={t('creation.taskPreview')}
        aria-busy={pendingPreview}
      >
        <header className="template-pane-heading">
          <strong title={preview?.config?.name}>
            {preview?.config?.name || t('creation.taskPreview')}
          </strong>
          <small className="quiet" role="status">
            {t(
              pendingPreview
                ? 'taskTemplate.updating'
                : current
                  ? 'creation.ready'
                  : 'creation.incomplete',
            )}
          </small>
        </header>
        <div className="template-pane-scroll">
          {failure && (
            <ErrorToast>
              {failure}
              <Button onClick={instance.refreshPreview}>{t('重试')}</Button>
            </ErrorToast>
          )}
          {issues.length > 0 && <TemplateIssues issues={issues} onSelect={focusIssue} />}
          {preview?.config && (
            <div className="template-preview-context">
              <ScheduleSummary
                schedule={preview.config.schedule}
                enabled={preview.config.enabled}
              />
              {generated && (
                <span className="quiet">
                  {t('dag.nodeCount', { count: generated.nodes.length })} ·{' '}
                  {t('moments.outputs', { count: outputs(generated).length })}
                </span>
              )}
            </div>
          )}
          {generated && (
            <>
              <DagGraph
                compact
                hideTitle
                nodes={generated.nodes}
                runners={allRunners}
                selected={selected}
                onSelect={setSelected}
                outputIds={outputs(generated)}
              />
            </>
          )}
          {selectedNode && (
            <section className="template-generated-instruction">
              <div className="template-node-heading">
                <strong>{selectedNode.name}</strong>
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
              </dl>
              <PromptPreview key={selectedNode.id} text={promptText(selectedNode.prompt)} />
            </section>
          )}
          {!generated && preview?.config && (
            <section className="template-generated-instruction">
              <dl className="properties">
                <dt>{t('设备')}</dt>
                <dd>
                  {allRunners.find(
                    (runner) =>
                      runner.id === preview.config!.runner ||
                      runner.name === preview.config!.runner,
                  )?.name || preview.config.runner}
                </dd>
                <dt>Agent</dt>
                <dd>{preview.config.agent}</dd>
                <dt>{t('工作目录')}</dt>
                <dd>{preview.config.cwd}</dd>
              </dl>
              <PromptPreview text={preview.config.prompt} />
            </section>
          )}
          {!preview && !failure && !issues.length && (
            <p className="quiet padded">
              {t(pendingPreview ? 'taskTemplate.updating' : 'taskTemplate.fillInputs')}
            </p>
          )}
        </div>
      </section>
    </div>
  )
}
function PromptPreview({ text }: { text: string }) {
  const toast = useToast()
  const [full, setFull] = useState(false)
  const lines = text.split('\n')
  return (
    <>
      <div className="template-instruction-tools">
        <span>{t('指令')}</span>
        <CopyValue value={text} toast={toast} />
      </div>
      <pre className="template-prompt">{full ? text : lines.slice(0, 12).join('\n')}</pre>
      {lines.length > 12 && (
        <Button onClick={() => setFull((value) => !value)}>{t(full ? '收起' : '展开全文')}</Button>
      )}
    </>
  )
}
function TemplateIssues({
  issues,
  onSelect,
}: {
  issues: TemplateIssue[]
  onSelect: (issue: TemplateIssue) => void
}) {
  return (
    <ul className="template-issues" aria-label={t('taskTemplate.validation')}>
      {issues.map((issue, index) => (
        <li key={`${issue.path}-${index}`}>
          <button type="button" onClick={() => onSelect(issue)}>
            <span>{issueText(issue)}</span>
            <code>{issue.path}</code>
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
