import YAML from 'yaml'
import { useEffect, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, ApiError, type Runner, type TaskConfig } from '../api'
import { t } from '../i18n'
import { Button, IconButton, Modal, Field } from '../ui'
import { ActionIcon } from '../actions/ActionIcon'
import { useToast } from '../Toast'
import { Pagination } from '../Pagination'
import { ChoicePicker } from '../ChoicePicker'
import { ModelPicker, ParameterFields } from '../ModelPicker'
import { DagGraph } from '../dag/DagGraph'
import { promptText } from '../dag/prompt'
import { outputs } from '../dag/model'
import { TimeFields } from '../dag/TimeFields'
import { InputEditor } from './InputEditor'
import { AuthorEditor, starter } from './AuthorEditor'
import { parseJsonDocument, parseOverrides } from './document'
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
const execution: TaskConfig['execution'] = {
  timeout: null,
  start_grace: '1h',
  overlap: 'skip',
  misfire: 'skip',
}
function definitionFrom(source: string): Definition {
  const value = parseJsonDocument(source) as unknown as Definition
  if (
    !value ||
    typeof value !== 'object' ||
    !value.task ||
    typeof value.task !== 'object' ||
    !value.inputs ||
    !value.bindings ||
    typeof value.bindings !== 'object' ||
    !value.inputs.properties ||
    typeof value.inputs.properties !== 'object'
  )
    throw new Error('taskTemplate.invalidDefinition')
  return value
}
export function issueText(issue: TemplateIssue): string {
  const key = 'taskTemplate.issue.' + issue.code,
    translated = t(key, issue.params)
  return translated === key ? issue.message : translated
}
export function TemplateDialog({
  seedTask,
  timezone,
  onClose,
  onCreated,
}: {
  seedTask?: string
  timezone: string
  onClose: () => void
  onCreated: (result: { task_id: string; run_id: string | null }) => void
}) {
  useTranslation()
  const client = useQueryClient(),
    toast = useToast()
  const [mode, setMode] = useState<'choose' | 'instance' | 'author'>(seedTask ? 'author' : 'choose')
  const [record, setRecord] = useState<TemplateRecord | null>(null),
    [source, setSource] = useState(''),
    [sourceError, setSourceError] = useState('')
  const [search, setSearch] = useState(''),
    [archived, setArchived] = useState(false),
    [page, setPage] = useState(1)
  const [step, setStep] = useState<'task' | 'time'>('task'),
    [name, setName] = useState('')
  const [values, setValues] = useState<Overrides>(blank),
    [valueMode, setValueMode] = useState<'form' | 'json' | 'yaml'>('form'),
    [rawValues, setRawValues] = useState(''),
    [valueError, setValueError] = useState('')
  const [bindings, setBindings] = useState<Record<string, Binding>>({}),
    [schedule, setSchedule] = useState<TaskConfig['schedule'] | undefined>(undefined)
  const [preview, setPreview] = useState<Preview | null>(null),
    [previewSignature, setPreviewSignature] = useState(''),
    [pendingPreview, setPendingPreview] = useState(false),
    [failure, setFailure] = useState(''),
    [actionIssues, setActionIssues] = useState<TemplateIssue[]>([])
  const [pending, setPending] = useState(false),
    [selected, setSelected] = useState('')
  const [taskId, setTaskId] = useState(() => crypto.randomUUID())
  const generation = useRef(0),
    operation = useRef<{ signature: string; key: string } | null>(null)
  const list = useQuery<{ items: TemplateRecord[]; total: number }>({
    queryKey: ['task-templates', search, archived, page],
    queryFn: () =>
      api(`/task-templates?q=${encodeURIComponent(search)}&archived=${archived}&page=${page}`),
    enabled: mode === 'choose',
  })
  const runners = useQuery<Runner[]>({
    queryKey: ['runners'],
    queryFn: () => api('/runners'),
    refetchInterval: 10000,
  })
  useEffect(() => {
    if (!seedTask) return
    let live = true
    api<Definition>(`/tasks/${encodeURIComponent(seedTask)}/template`)
      .then((def) => {
        if (live) setSource(YAML.stringify(def, { lineWidth: 0 }))
      })
      .catch((e) => {
        if (live) toast(e.message)
      })
    return () => {
      live = false
    }
  }, [seedTask])
  let definition: Definition | null = null,
    parseError = sourceError
  try {
    if (mode === 'author' && source) definition = definitionFrom(source)
    else if (record) definition = record.definition
  } catch (e) {
    parseError = (e as Error).message
  }
  const request: Request = {
    ...values,
    bindings,
    expected_revision: record?.revision ?? 0,
    task_id: taskId,
    ...(name ? { name } : {}),
    ...(schedule !== undefined ? { schedule } : {}),
  }
  const signature = JSON.stringify({
    mode,
    definition,
    request,
    raw: valueError ? rawValues : null,
    parseError,
    valueError,
  })
  const current = previewSignature === signature && !parseError && !valueError
  useEffect(() => {
    const seq = ++generation.current,
      controller = new AbortController()
    setFailure('')
    setActionIssues([])
    if (mode === 'choose' || !definition || parseError || valueError) {
      setPendingPreview(false)
      return
    }
    setPendingPreview(true)
    const timer = setTimeout(async () => {
      try {
        const result = await api<Preview>(
          mode === 'author'
            ? '/task-template-preview'
            : `/task-templates/${encodeURIComponent(record!.id)}/preview`,
          'POST',
          mode === 'author'
            ? { definition, request: { ...request, request_id: String(seq) } }
            : { ...request, request_id: String(seq) },
          undefined,
          controller.signal,
        )
        if (seq !== generation.current || controller.signal.aborted) return
        setPreview(result)
        setPreviewSignature(signature)
        setSelected((id) =>
          result.graph?.nodes.some((n) => n.id === id) ? id : (result.graph?.nodes[0]?.id ?? ''),
        )
      } catch (e) {
        if (!controller.signal.aborted && seq === generation.current) {
          setFailure((e as Error).message)
          if (e instanceof ApiError)
            setActionIssues((e.body as { issues?: TemplateIssue[] })?.issues ?? [])
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
  function choose(next: TemplateRecord) {
    setRecord(next)
    setMode('instance')
    setSource('')
    setSourceError('')
    setValues(blank)
    setBindings({})
    setSchedule(undefined)
    setName('')
    setStep('task')
    setValueMode('form')
    setValueError('')
    setPreview(null)
    setTaskId(crypto.randomUUID())
    operation.current = null
  }
  function raw(mode: 'form' | 'json' | 'yaml') {
    if (valueError && mode === 'form') return
    setValueMode(mode)
    if (mode !== 'form')
      setRawValues(
        mode === 'json'
          ? JSON.stringify(values, null, 2)
          : YAML.stringify(values, { lineWidth: 0 }),
      )
  }
  async function save() {
    if (!definition) return
    setPending(true)
    setActionIssues([])
    try {
      const next = await api<TemplateRecord>(
        `/task-templates/${encodeURIComponent(definition.id)}`,
        'PUT',
        { definition, expected_revision: record?.id === definition.id ? record.revision : null },
      )
      client.invalidateQueries({ queryKey: ['task-templates'] })
      toast(t('taskTemplate.saved'))
      choose(next)
    } catch (e) {
      toast((e as Error).message)
      if (e instanceof ApiError)
        setActionIssues((e.body as { issues?: TemplateIssue[] })?.issues ?? [])
    } finally {
      setPending(false)
    }
  }
  async function availability(target: TemplateRecord) {
    setPending(true)
    try {
      await api(
        `/task-templates/${encodeURIComponent(target.id)}/${target.archived_at ? 'restore' : 'archive'}`,
        'POST',
        { expected_revision: target.revision },
      )
      client.invalidateQueries({ queryKey: ['task-templates'] })
    } catch (e) {
      toast((e as Error).message)
    } finally {
      setPending(false)
    }
  }
  async function create(run_now: boolean) {
    if (!record || !current || !preview?.config) return
    const body = { ...request, run_now },
      keySignature = JSON.stringify(body)
    if (operation.current?.signature !== keySignature)
      operation.current = { signature: keySignature, key: crypto.randomUUID() }
    setPending(true)
    setActionIssues([])
    try {
      const result = await api<{ task_id: string; run_id: string | null }>(
        `/task-templates/${encodeURIComponent(record.id)}/instantiate`,
        'POST',
        body,
        operation.current.key,
      )
      client.invalidateQueries({ queryKey: ['tasks'] })
      client.invalidateQueries({ queryKey: ['runs'] })
      onCreated(result)
    } catch (e) {
      toast((e as Error).message)
      if (e instanceof ApiError)
        setActionIssues((e.body as { issues?: TemplateIssue[] })?.issues ?? [])
    } finally {
      setPending(false)
    }
  }
  function beginAuthor(dag: boolean) {
    setRecord(null)
    setSource(YAML.stringify(starter(dag), { lineWidth: 0 }))
    setMode('author')
    setPreview(null)
    setSourceError('')
  }
  const allRunners = (runners.data ?? []).filter((r) => !r.revoked)
  const knownBindings = definition?.bindings ?? {}
  const visibleBindings = preview?.used_bindings.length
    ? Object.keys(knownBindings).filter((k) => preview.used_bindings.includes(k))
    : Object.keys(knownBindings)
  const generated = preview?.graph,
    selectedNode = generated?.nodes.find((n) => n.id === selected)
  const issues = [...(preview?.issues ?? []), ...actionIssues]
  const timeConfig: TaskConfig = preview?.config ?? {
    version: 1,
    id: taskId,
    name,
    runner: '',
    agent: '',
    cwd: '',
    prompt: '',
    parameters: {},
    enabled: true,
    schedule: null,
    execution,
    ...(definition?.task.dag ? { dag: generated ?? { nodes: [] } } : {}),
  }
  return (
    <Modal
      open
      title={t(
        mode === 'choose'
          ? 'taskTemplate.fromTemplate'
          : mode === 'author'
            ? 'taskTemplate.editTemplate'
            : 'taskTemplate.createTask',
      )}
      wide
      className="template-modal"
      onOpenChange={(v) => {
        if (!v && !pending) onClose()
      }}
    >
      <fieldset disabled={pending} className="template-workspace">
        {mode === 'choose' ? (
          <>
            <div className="template-toolbar">
              <input
                type="search"
                aria-label={t('taskTemplate.search')}
                placeholder={t('taskTemplate.search')}
                value={search}
                onChange={(e) => {
                  setSearch(e.target.value)
                  setPage(1)
                }}
              />
              <label>
                <input
                  type="checkbox"
                  checked={archived}
                  onChange={(e) => {
                    setArchived(e.target.checked)
                    setPage(1)
                  }}
                />
                {t('已归档')}
              </label>
              <Button onClick={() => beginAuthor(false)}>{t('taskTemplate.newTemplate')}</Button>
              <Button onClick={() => beginAuthor(true)}>{t('taskTemplate.dynamicDag')}</Button>
            </div>
            <div className="template-list">
              {list.isPending ? (
                <p className="quiet">{t('正在加载…')}</p>
              ) : list.error ? (
                <p role="alert">{list.error.message}</p>
              ) : !list.data?.items.length ? (
                <p className="quiet">{t('taskTemplate.empty')}</p>
              ) : (
                list.data.items.map((row) => (
                  <div className="template-list-row" key={row.id}>
                    <button
                      className="template-list-name"
                      disabled={!!row.archived_at}
                      onClick={() => choose(row)}
                    >
                      <strong>{row.definition.name}</strong>
                      <small>
                        {row.definition.task.dag ? t('dag.task') : t('任务')} ·{' '}
                        {t('taskOverview.parameterCount', {
                          count: Object.keys(row.definition.inputs.properties ?? {}).length,
                        })}{' '}
                        · {row.definition.description || '—'}
                      </small>
                    </button>
                    <small className="quiet">r{row.revision}</small>
                    <IconButton
                      label={t('taskTemplate.editTemplate')}
                      onClick={() => {
                        setRecord(row)
                        setSource(YAML.stringify(row.definition, { lineWidth: 0 }))
                        setMode('author')
                        setValues(blank)
                        setBindings({})
                        setSchedule(undefined)
                      }}
                    >
                      <ActionIcon name="edit" />
                    </IconButton>
                    <IconButton
                      label={t(row.archived_at ? '恢复' : '归档')}
                      onClick={() => void availability(row)}
                    >
                      <ActionIcon name={row.archived_at ? 'restore' : 'archive'} />
                    </IconButton>
                  </div>
                ))
              )}
            </div>
            <Pagination page={page} total={list.data?.total ?? 0} onChange={setPage} />
          </>
        ) : (
          <>
            <header className="template-toolbar">
              <IconButton label={t('返回')} onClick={() => setMode('choose')}>
                <ActionIcon name="back" />
              </IconButton>
              <strong>{definition?.name || t('taskTemplate.newTemplate')}</strong>
              {record && <small className="quiet">r{record.revision}</small>}
              <span className="template-spacer" />
              {mode === 'instance' && (
                <>
                  <Button aria-pressed={step === 'task'} onClick={() => setStep('task')}>
                    {t('任务')}
                  </Button>
                  <Button aria-pressed={step === 'time'} onClick={() => setStep('time')}>
                    {t('时间')}
                  </Button>
                  <IconButton
                    label={t('taskTemplate.editTemplate')}
                    onClick={() => {
                      setSource(YAML.stringify(record!.definition, { lineWidth: 0 }))
                      setMode('author')
                    }}
                  >
                    <ActionIcon name="edit" />
                  </IconButton>
                </>
              )}
            </header>
            <div className={`template-columns ${mode === 'author' ? 'is-author' : ''}`}>
              <div className="template-controls">
                {mode === 'author' ? (
                  <>
                    <AuthorEditor
                      source={source}
                      definition={definition}
                      error={parseError}
                      onSource={(next) => {
                        setSource(next)
                        setSourceError('')
                      }}
                    />
                    {definition && (
                      <details className="template-test-inputs">
                        <summary>{t('taskTemplate.testInputs')}</summary>
                        <InputEditor
                          schema={definition.inputs}
                          value={values}
                          onChange={setValues}
                          runners={allRunners}
                        />
                      </details>
                    )}
                  </>
                ) : step === 'time' ? (
                  <TimeFields
                    config={{
                      ...timeConfig,
                      schedule:
                        schedule === undefined
                          ? ((definition?.task.schedule as TaskConfig['schedule']) ?? null)
                          : schedule,
                    }}
                    timezone={timezone}
                    disabled={pending}
                    onChange={(c) => setSchedule(c.schedule)}
                  />
                ) : (
                  <>
                    <Field label={t('名称')}>
                      <input
                        aria-label={t('taskTemplate.taskName')}
                        value={name}
                        placeholder={preview?.config?.name ?? ''}
                        onChange={(e) => setName(e.target.value)}
                      />
                    </Field>
                    <div className="template-toolbar">
                      <h3>{t('taskTemplate.inputs')}</h3>
                      <span className="template-spacer" />
                      {(['form', 'json', 'yaml'] as const).map((m) => (
                        <Button
                          key={m}
                          aria-pressed={valueMode === m}
                          disabled={!!valueError && m === 'form'}
                          onClick={() => raw(m)}
                        >
                          {m === 'form' ? t('taskTemplate.form') : m.toUpperCase()}
                        </Button>
                      ))}
                    </div>
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
                          onChange={(e) => {
                            setRawValues(e.target.value)
                            try {
                              setValues(parseOverrides(e.target.value))
                              setValueError('')
                            } catch (error) {
                              setValueError((error as Error).message)
                            }
                          }}
                        />
                        {valueError && (
                          <p className="template-error" role="alert">
                            {t(valueError)}
                          </p>
                        )}
                      </>
                    )}
                  </>
                )}
                {definition && (
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
              </div>
              <div className="template-generated" aria-busy={pendingPreview}>
                <div className="template-toolbar">
                  <h3>{t('taskTemplate.preview')}</h3>
                  {generated && (
                    <small className="quiet">
                      {t('dag.nodeCount', { count: generated.nodes.length })} ·{' '}
                      {t('moments.outputs', { count: outputs(generated).length })}
                    </small>
                  )}
                  <span className="template-spacer" />
                  <small className="quiet" role="status">
                    {t(
                      pendingPreview
                        ? 'taskTemplate.updating'
                        : !current
                          ? 'taskTemplate.stale'
                          : 'taskTemplate.current',
                    )}
                  </small>
                </div>
                {failure && (
                  <p className="template-error" role="alert">
                    {failure}
                  </p>
                )}
                {generated && (
                  <DagGraph
                    compact
                    nodes={generated.nodes}
                    runners={allRunners}
                    selected={selected}
                    onSelect={setSelected}
                    outputIds={outputs(generated)}
                  />
                )}
                {selectedNode && (
                  <section className="template-generated-instruction">
                    <div className="template-toolbar">
                      <strong>{selectedNode.name}</strong>
                      <code>{selectedNode.id}</code>
                    </div>
                    <dl className="template-source-map">
                      <dt>{t('设备')}</dt>
                      <dd>
                        {allRunners.find(
                          (r) => r.id === selectedNode.runner || r.name === selectedNode.runner,
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
                    <pre>{promptText(selectedNode.prompt)}</pre>
                  </section>
                )}
                {!generated && preview?.config && (
                  <section className="template-generated-instruction">
                    <strong>{preview.config.name}</strong>
                    <p className="quiet mono">{preview.config.cwd}</p>
                    <pre>{preview.config.prompt}</pre>
                  </section>
                )}
                {!preview && <p className="quiet padded">{t('taskTemplate.fillInputs')}</p>}
                {issues.length > 0 && (
                  <ul className="template-issues" aria-label={t('taskTemplate.validation')}>
                    {issues.map((issue, i) => (
                      <li key={i}>
                        <button
                          type="button"
                          onClick={() => {
                            if (issue.node_id) setSelected(issue.node_id)
                            const name = /^inputs\.([^.\[]+)/.exec(issue.path)?.[1]
                            if (name) {
                              setStep('task')
                              requestAnimationFrame(() =>
                                document
                                  .querySelector<HTMLElement>(`[data-input="${CSS.escape(name)}"]`)
                                  ?.scrollIntoView({ block: 'nearest' }),
                              )
                            }
                          }}
                        >
                          <code>
                            {issue.path}
                            {issue.line ? `:${issue.line}:${issue.column ?? 1}` : ''}
                          </code>
                          <span>{issueText(issue)}</span>
                        </button>
                        {['render', 'expression', 'syntax', 'configuration', 'binding'].includes(
                          issue.code,
                        ) && (
                          <details>
                            <summary>{t('taskTemplate.details')}</summary>
                            <pre>{issue.message}</pre>
                          </details>
                        )}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            </div>
          </>
        )}
      </fieldset>
      <footer className="modal-footer">
        <Button disabled={pending} onClick={onClose}>
          {t('关闭')}
        </Button>
        {mode === 'author' ? (
          <Button
            variant="solid"
            loading={pending}
            disabled={!definition || !!parseError || !!record?.archived_at}
            onClick={() => void save()}
          >
            {t('taskTemplate.saveTemplate')}
          </Button>
        ) : mode === 'instance' ? (
          <>
            <Button
              disabled={pending || !current || !preview?.config || !!record?.archived_at}
              onClick={() => void create(false)}
            >
              {t('taskTemplate.createTask')}
            </Button>
            <Button
              variant="solid"
              loading={pending}
              disabled={!current || !preview?.config || !!record?.archived_at}
              onClick={() => void create(true)}
            >
              {t('taskTemplate.createAndRun')}
            </Button>
          </>
        ) : null}
      </footer>
    </Modal>
  )
}
function BindingFields({
  title,
  slot,
  defaults,
  value,
  runners,
  onChange,
}: {
  title: string
  slot: string
  defaults: Binding
  value: Binding
  runners: Runner[]
  onChange: (value: Binding) => void
}) {
  const [options, setOptions] = useState<Record<string, string[]>>({})
  const runnerId = value.runner ?? defaults.runner ?? '',
    agentId = value.agent ?? defaults.agent ?? '',
    parameters = value.parameters ?? defaults.parameters ?? {}
  const runner = runners.find((r) => r.id === runnerId || r.name === runnerId),
    agent = runner?.agents.find((a) => a.id === agentId)
  return (
    <div className="template-binding">
      <h4>{title}</h4>
      <Field label={t('设备')}>
        <ChoicePicker
          label={t('taskTemplate.runnerBinding', { name: title })}
          value={runnerId}
          searchable
          onChange={(runner) => onChange({ ...value, runner, agent: null, parameters: {} })}
          options={runners.map((r) => ({
            value: r.id,
            label: r.name || r.id,
            status: !r.online ? t('离线') : undefined,
          }))}
        />
      </Field>
      <Field label="Agent">
        <ChoicePicker
          label={t('taskTemplate.agentBinding', { name: title })}
          value={agentId}
          onChange={(agent) => onChange({ ...value, agent, parameters: {} })}
          options={(runner?.agents ?? []).map((a) => ({
            value: a.id,
            label: a.name,
            disabled: !a.available,
          }))}
        />
      </Field>
      <ModelPicker
        key={`${slot}-${runnerId}-${agentId}`}
        runner={runner}
        agent={agent}
        cwd={runner?.directory_roots[0] ?? ''}
        values={parameters}
        onChange={(parameters) => onChange({ ...value, parameters })}
        onOptionsChange={setOptions}
      />
      <ParameterFields
        agent={agent}
        values={parameters}
        options={options}
        onChange={(parameters) => onChange({ ...value, parameters })}
      />
    </div>
  )
}
