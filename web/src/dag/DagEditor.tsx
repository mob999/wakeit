import { ErrorToast, useToast } from '../Toast'
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useBlocker } from '@tanstack/react-router'
import * as Menu from '@radix-ui/react-dropdown-menu'
import * as Popover from '@radix-ui/react-popover'
import {
  Plus,
  MoreHorizontal,
  ChevronDown,
  GitBranch,
  List,
  Search,
  ArrowUpRight,
} from 'lucide-react'
import {
  api,
  type Task,
  type TaskConfig,
  type Runner,
  type Agent,
  type DagIssue,
  type DagNode,
} from '../api'
import { t } from '../i18n'
import { Button, IconButton, Field, Modal } from '../ui'
import { EditorMemoryContext, type EditorMemory } from '../EditorMemory'
import { PageBreadcrumbs } from '../PageHeader'
import { useSearchControls } from '../Search'
import { createTaskId, taskIdSuffix } from '../taskId'
import { ChoicePicker } from '../ChoicePicker'
import { DagGraph, type DagView } from './DagGraph'
import { NodeForm } from './NodeForm'
import { TimeFields } from './TimeFields'
import { MultiPicker } from './MultiPicker'
import { newNode, issues, referenceLocations, outputs } from './model'
import { nodeName, nodeOptions, shortId } from './identity'
import { issueField, locateField, markFields } from './fields'
import { validDuration } from '../duration'
function valueAt(config: TaskConfig, issue: DagIssue) {
  const base = issue.node_id ? config.dag?.nodes.find((n) => n.id === issue.node_id) : config
  return issueField(issue)
    .split('.')
    .reduce<any>((value, key) => value?.[key], base)
}
export function DagEditor({
  task,
  timezone,
  onClose,
  onSaved,
}: {
  task?: Task
  timezone: string
  onClose: () => void
  onSaved: (id: string) => void
}) {
  useTranslation()
  const [suffix] = useState(taskIdSuffix)
  const [config, setConfig] = useState<TaskConfig>(() =>
    task
      ? structuredClone({
          ...task.config,
          runner: '',
          agent: '',
          cwd: '',
          prompt: '',
          parameters: {},
        })
      : {
          version: 1,
          id: '',
          name: '',
          runner: '',
          agent: '',
          cwd: '',
          prompt: '',
          parameters: {},
          enabled: true,
          schedule: { cron: '0 9 * * *', timezone },
          execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
          dag: { nodes: [newNode()] },
        },
  )
  const skipMenuFocus = useRef(false)
  const menuFocusTarget = useRef<DagIssue | null>(null)
  const baseline = useRef(JSON.stringify(config)),
    editor = useRef<HTMLDivElement>(null),
    viewport = useRef<HTMLDivElement>(null)
  const [selected, setSelected] = useState(config.dag!.nodes[0]?.id ?? ''),
    [expanded, setExpanded] = useState<string | null>(
      task ? null : (config.dag!.nodes[0]?.id ?? null),
    )
  const [step, setStep] = useState<'task' | 'time'>('task'),
    [view, setView] = useState<DagView>('graph'),
    [search, setSearch] = useState('')
  const [location, setLocation] = useState<{ id: string; sequence: number }>()
  const [memory, setMemory] = useState<Record<string, EditorMemory>>({})
  const [serverErrors, setServerErrors] = useState<DagIssue[]>([]),
    [validated, setValidated] = useState(false),
    [requestError, setRequestError] = useState('')
  const [focus, setFocus] = useState<DagIssue | null>(null),
    [reveal, setReveal] = useState(false),
    [removing, setRemoving] = useState<string | null>(null)
  const [leave, setLeave] = useState(false),
    leaveResolve = useRef<((value: boolean) => void) | null>(null)
  const timeValidation = useRef<() => DagIssue[]>(() => []),
    nextError = useRef(0)
  const client = useQueryClient(),
    toast = useToast(),
    controls = useSearchControls()
  const runners = useQuery<Runner[]>({
    queryKey: ['runners'],
    queryFn: () => api('/runners'),
    refetchInterval: 3000,
  })
  const agents = (runners.data ?? []).flatMap((r) =>
    r.agents.map((a) => ({ ...a, runner: r.name, runner_id: r.id })),
  ) as Agent[]
  const dag = config.dag!,
    node = dag.nodes.find((n) => n.id === expanded)
  const contexts = JSON.stringify(
    dag.nodes.map((n) => {
      const r = runners.data?.find((r) => r.id === n.runner || r.name === n.runner)
      return [n.id, n.runner, n.agent, n.cwd, r?.agents.find((a) => a.id === n.agent)?.digest]
    }),
  )
  useEffect(() => {
    const current: unknown[][] = JSON.parse(contexts)
    setMemory((previous) => {
      let changed = false
      const next = { ...previous }
      for (const [id, ...context] of current) {
        const key = String(id),
          fingerprint = JSON.stringify(context),
          old = previous[key]
        if (!old || old.context === fingerprint) continue
        changed = true
        next[key] = Object.fromEntries(
          Object.entries(old).filter(
            ([key]) => !key.startsWith('model.') && !key.startsWith('session.'),
          ),
        )
        next[key].context = fingerprint
      }
      return changed ? next : previous
    })
  }, [contexts])
  const positions = useRef<Record<string, number>>({})
  const stepScroll = useRef({ task: 0, time: 0 })
  useLayoutEffect(() => {
    if (viewport.current) viewport.current.scrollTop = stepScroll.current[step]
  }, [step])
  const dirty = JSON.stringify(config) !== baseline.current,
    dirtyRef = useRef(dirty),
    busyRef = useRef(false)
  dirtyRef.current = dirty
  const requestLeave = useCallback(() => {
    if (busyRef.current) return Promise.resolve(false)
    if (!dirtyRef.current) return Promise.resolve(true)
    setLeave(true)
    return new Promise<boolean>((resolve) => {
      leaveResolve.current = resolve
    })
  }, [])
  useEffect(() => controls.registerGuard(requestLeave), [controls.registerGuard, requestLeave])
  useBlocker({
    shouldBlockFn: () => requestLeave().then((value) => !value),
    enableBeforeUnload: () => dirtyRef.current,
  })
  useEffect(
    () => () => {
      leaveResolve.current?.(false)
    },
    [],
  )
  function change(next: TaskConfig) {
    setServerErrors((old) =>
      old.filter(
        (issue) => JSON.stringify(valueAt(config, issue)) === JSON.stringify(valueAt(next, issue)),
      ),
    )
    setConfig(next)
  }
  const changeDag = (next: typeof dag) => change({ ...config, dag: next })
  function localErrors(): DagIssue[] {
    const result = issues(dag)
    if (!config.name.trim()) result.unshift({ path: 'name', message: t('请填写任务名称') })
    for (const n of dag.nodes) {
      const r = runners.data?.find((r) => r.id === n.runner || r.name === n.runner),
        a = r?.agents.find((a) => a.id === n.agent)
      const add = (field: string, message: string) =>
        result.push({ node_id: n.id, path: 'dag.nodes.' + n.id + '.' + field, message })
      if (n.runner && runners.data && (!r || r.revoked)) add('runner', t('请选择 Runner'))
      if (r && n.agent && !a) add('agent', t('请选择该 Runner 的 Agent'))
      if (!validDuration(n.timeout)) add('timeout', t('请填写有效的执行超时（1 秒至 366 天）'))
      if (n.session && (!n.session.id.trim() || !a?.session_supported))
        add('session.id', t('该 Agent 不支持绑定会话'))
      if (a)
        for (const [key, value] of Object.entries(n.parameters)) {
          const spec = a.parameters[key]
          if (!spec || (value && spec.enum?.length && !spec.enum.includes(value)))
            add('parameters.' + key, t('参数无效：{{v0}}', { v0: spec?.label || key }))
        }
    }
    return result
  }
  const local = validated ? localErrors() : [],
    errors = [...local, ...serverErrors.filter((e) => !local.some((l) => l.path === e.path))]
  function openNode(id: string, toggle = false) {
    if (busyRef.current) return
    setSelected(id)
    setExpanded(toggle && expanded === id ? null : id)
    setReveal(true)
  }
  function locateNode(id: string) {
    setSearch('')
    openNode(id)
    setLocation((previous) => ({ id, sequence: (previous?.sequence ?? 0) + 1 }))
  }
  function locate(issue: DagIssue) {
    setStep(
      issue.path.startsWith('schedule') || issue.path.startsWith('execution.') ? 'time' : 'task',
    )
    if (issue.node_id) {
      setSelected(issue.node_id)
      setExpanded(issue.node_id)
      setSearch('')
    }
    setFocus(issue)
    if (issue.message)
      toast(issue.message + (issue.cycle?.length ? ' (' + issue.cycle.join(' → ') + ')' : ''))
  }
  const save = useMutation({
    mutationFn: async () => {
      setRequestError('')
      setValidated(true)
      const found = [...localErrors(), ...timeValidation.current()]
      if (found.length) {
        setServerErrors(found.filter((e) => !e.node_id && e.path !== 'name'))
        locate(found[0])
        return null
      }
      const next = { ...config, id: task?.id ?? createTaskId(config.name, suffix) }
      const validation = await api<{ errors: DagIssue[] }>('/tasks/validate', 'POST', {
        config: next,
      })
      setServerErrors(validation.errors)
      if (validation.errors.length) {
        locate(validation.errors[0])
        return null
      }
      return api<Task>('/tasks/' + encodeURIComponent(next.id), 'PUT', {
        config: next,
        expected_revision: task?.revision ?? null,
      })
    },
    onSuccess: (result) => {
      if (!result) return
      baseline.current = JSON.stringify(config)
      dirtyRef.current = false
      busyRef.current = false
      client.invalidateQueries({ queryKey: ['tasks'] })
      onSaved(result.id)
    },
    onError: (error: Error) => setRequestError(error.message),
  })
  const busy = save.isPending
  busyRef.current = busy
  useLayoutEffect(() => {
    markFields(
      editor.current,
      errors.filter((e) => !e.node_id),
    )
    if (busy) return
    if (focus) {
      locateField(editor.current, focus)
      setFocus(null)
    } else if (reveal && expanded) {
      const panel = editor.current?.querySelector<HTMLElement>(
          '[data-expanded-node="' + expanded + '"]',
        ),
        pane = viewport.current
      if (panel && pane) {
        const r = panel.getBoundingClientRect(),
          p = pane.getBoundingClientRect()
        if (r.top < p.top || r.top > p.bottom - 100)
          pane.scrollTop = r.top - p.top + pane.scrollTop + (positions.current[expanded] ?? 0)
      }
      setReveal(false)
    }
  })
  function add(source?: DagNode, copy = false) {
    if (source) skipMenuFocus.current = true
    const next = newNode(dag.nodes)
    if (source)
      Object.assign(next, {
        runner: source.runner,
        agent: source.agent,
        cwd: source.cwd,
        parameters: structuredClone(source.parameters),
        timeout: source.timeout,
        pre: copy ? [...source.pre] : [source.id],
        ...(copy
          ? {
              prompt: source.prompt,
              session: structuredClone(source.session),
              name: source.name + ' ' + t('dag.copySuffix'),
            }
          : {}),
      })
    changeDag({ ...dag, nodes: [...dag.nodes, next] })
    setSelected(next.id)
    setExpanded(next.id)
    setSearch('')
    const target = { node_id: next.id, path: 'dag.nodes.' + next.id + '.name', message: '' }
    if (source) menuFocusTarget.current = target
    if (!source) setFocus(target)
  }
  function nodeActions(n: DagNode) {
    return (
      <Menu.Root>
        <Menu.Trigger asChild>
          <IconButton label={t('dag.nodeActions')} disabled={busy}>
            <MoreHorizontal />
          </IconButton>
        </Menu.Trigger>
        <Menu.Portal>
          <Menu.Content
            className="dropdown"
            align="end"
            onCloseAutoFocus={(event) => {
              if (skipMenuFocus.current) {
                event.preventDefault()
                skipMenuFocus.current = false
                const target = menuFocusTarget.current
                menuFocusTarget.current = null
                if (target) locateField(editor.current, target)
              }
            }}
          >
            <Menu.Item disabled={busy} onSelect={() => add(n)}>
              {t('dag.addNext')}
            </Menu.Item>
            <Menu.Item disabled={busy} onSelect={() => add(n, true)}>
              {t('dag.duplicate')}
            </Menu.Item>
            <Menu.Item
              disabled={busy}
              onSelect={() => {
                setRemoving(n.id)
                if (referenceLocations(dag, n.id).length) toast(t('dag.deleteBlocked'))
              }}
            >
              {t('dag.delete')}
            </Menu.Item>
          </Menu.Content>
        </Menu.Portal>
      </Menu.Root>
    )
  }
  const inline = node && (
    <div className="dag-expanded-node" data-expanded-node={node.id}>
      <div className="dag-node-context">
        <IconButton label={t('dag.collapseNode')} onClick={() => setExpanded(null)} disabled={busy}>
          <ChevronDown />
        </IconButton>
        {dag.nodes.length > 1 ? (
          <ChoicePicker
            label={t('dag.selectNode')}
            searchable
            disabled={busy}
            value={node.id}
            options={nodeOptions(dag.nodes).map((o) => ({
              value: o.id,
              label: o.name,
              description: o.description,
              keywords: o.id,
            }))}
            onChange={(id) => openNode(id)}
          />
        ) : (
          <strong>{nodeName(node)}</strong>
        )}
        {nodeActions(node)}
      </div>
      <EditorMemoryContext.Provider
        value={{
          active: step === 'task',
          value: memory[node.id] ?? {},
          update: (update) =>
            setMemory((previous) => ({
              ...previous,
              [node.id]: {
                ...(typeof update === 'function' ? update(previous[node.id] ?? {}) : update),
                context: JSON.stringify(
                  JSON.parse(contexts)
                    .find((entry: unknown[]) => entry[0] === node.id)
                    ?.slice(1),
                ),
              },
            })),
        }}
      >
        <NodeForm
          key={node.id}
          node={node}
          dag={dag}
          runners={runners.data ?? []}
          disabled={busy}
          errors={errors}
          onLocate={locateNode}
          onChange={(next) => {
            changeDag({ ...dag, nodes: dag.nodes.map((n) => (n.id === next.id ? next : n)) })
          }}
        />
      </EditorMemoryContext.Provider>
    </div>
  )
  const references = removing ? referenceLocations(dag, removing) : []
  return (
    <div className="dag-editor" ref={editor}>
      <PageBreadcrumbs
        label={t('Tasks')}
        crumbs={[{ label: task ? task.config.name : t('dag.new') }]}
        onBack={onClose}
      />
      <div className="dag-stepbar">
        <div className="tabs" role="tablist" aria-label={t('任务编辑')}>
          {(['task', 'time'] as const).map((value) => (
            <button
              type="button"
              key={value}
              role="tab"
              aria-selected={step === value}
              className={step === value ? 'active' : ''}
              disabled={busy}
              onClick={() => setStep(value)}
            >
              {t(value === 'task' ? '任务' : '时间')}
            </button>
          ))}
        </div>
        <div className="dag-editor-tools" hidden={step !== 'task'}>
          <IconButton
            label={t('dag.graph')}
            aria-pressed={view === 'graph'}
            disabled={busy}
            onClick={() => setView('graph')}
          >
            <GitBranch />
          </IconButton>
          <IconButton
            label={t('dag.list')}
            aria-pressed={view === 'list'}
            disabled={busy}
            onClick={() => setView('list')}
          >
            <List />
          </IconButton>
          <Popover.Root>
            <Popover.Trigger asChild>
              <IconButton label={t('dag.search')} disabled={busy}>
                <Search />
              </IconButton>
            </Popover.Trigger>
            <Popover.Portal>
              <Popover.Content className="dag-search-popover" align="end" sideOffset={6}>
                <input
                  aria-label={t('dag.search')}
                  placeholder={t('dag.search')}
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                />
                {search && (
                  <div className="dag-search-results">
                    {dag.nodes
                      .filter((n) =>
                        (nodeName(n) + ' ' + n.id).toLowerCase().includes(search.toLowerCase()),
                      )
                      .map((n) => (
                        <Popover.Close asChild key={n.id}>
                          <button
                            disabled={busy}
                            onClick={() => {
                              locateNode(n.id)
                            }}
                          >
                            {nodeName(n)}
                            <small className="mono">{shortId(n.id)}</small>
                          </button>
                        </Popover.Close>
                      ))}
                  </div>
                )}
              </Popover.Content>
            </Popover.Portal>
          </Popover.Root>
          <IconButton label={t('dag.addNode')} disabled={busy} onClick={() => add()}>
            <Plus />
          </IconButton>
        </div>
      </div>
      <div
        className="dag-editor-scroll"
        ref={viewport}
        onScroll={(e) => {
          stepScroll.current[step] = e.currentTarget.scrollTop
          if (step !== 'task' || !expanded) return
          const panel = e.currentTarget.querySelector<HTMLElement>(
            '[data-expanded-node="' + expanded + '"]',
          )
          if (panel)
            positions.current[expanded] = Math.max(
              0,
              e.currentTarget.getBoundingClientRect().top - panel.getBoundingClientRect().top,
            )
        }}
      >
        <div hidden={step !== 'task'} inert={step !== 'task'}>
          <section className="content-card dag-name-card">
            <Field label={t('任务名称')} fieldKey="name">
              <input
                autoFocus={!task}
                disabled={busy}
                value={config.name}
                onChange={(e) => change({ ...config, name: e.target.value })}
              />
            </Field>
          </section>
          <section className="content-card dag-nodes-card">
            {view === 'graph' && (
              <header className="card-header">
                <h3>{t('dag.nodes')}</h3>
              </header>
            )}
            <DagGraph
              nodes={dag.nodes}
              runners={runners.data}
              agents={agents}
              selected={selected}
              onSelect={(id) => openNode(id, true)}
              view={view}
              search={search}
              hideHeader
              disabled={busy}
              invalid={errors.flatMap((e) => (e.node_id ? [e.node_id] : []))}
              outputIds={outputs(dag)}
              inlineNodeId={expanded}
              inline={inline}
              rowActions={nodeActions}
              locateRequest={location}
            />
            {runners.error && <ErrorToast error={runners.error} />}
          </section>
          <section className="content-card dag-output-card" data-field="dag.results">
            <header className="card-header">
              <h3>{t('结果')}</h3>
            </header>
            <div className="dag-settings-rows">
              <Field label={t('dag.resultsMode')}>
                <ChoicePicker
                  label={t('dag.resultsMode')}
                  disabled={busy || !dag.nodes.length}
                  value={dag.results == null ? 'auto' : 'custom'}
                  options={[
                    { value: 'auto', label: t('dag.autoResults') },
                    { value: 'custom', label: t('dag.customResults') },
                  ]}
                  onChange={(value) =>
                    changeDag({ ...dag, results: value === 'auto' ? undefined : [...outputs(dag)] })
                  }
                />
              </Field>
              {dag.results != null && (
                <Field label={t('dag.results')}>
                  <MultiPicker
                    label={t('dag.results')}
                    disabled={busy}
                    minimum={1}
                    value={dag.results}
                    options={nodeOptions(dag.nodes)}
                    onChange={(results) => changeDag({ ...dag, results })}
                  />
                </Field>
              )}
              <div className="dag-output-rows">
                {dag.nodes
                  .filter((n) => outputs(dag).includes(n.id))
                  .map((n) => (
                    <div key={n.id}>
                      <span>{nodeName(n)}</span>
                      <span className="quiet mono">{shortId(n.id)}</span>
                      <IconButton
                        label={t('dag.locateSource', { name: nodeName(n) })}
                        disabled={busy}
                        onClick={() => {
                          locateNode(n.id)
                        }}
                      >
                        <ArrowUpRight />
                      </IconButton>
                    </div>
                  ))}
              </div>
            </div>
          </section>
        </div>
        <div hidden={step !== 'time'} inert={step !== 'time'}>
          <TimeFields
            config={config}
            timezone={timezone}
            disabled={busy}
            active={step === 'time'}
            onChange={change}
            validation={timeValidation}
          />
        </div>
      </div>
      <footer className="dag-editor-footer">
        {requestError && <ErrorToast>{requestError}</ErrorToast>}
        {errors.length > 0 && (
          <Button
            disabled={busy}
            onClick={() => {
              locate(errors[nextError.current++ % errors.length])
            }}
          >
            {t('dag.nextIssue')}
          </Button>
        )}
        <Button
          disabled={busy}
          onClick={async () => {
            if (await requestLeave()) {
              dirtyRef.current = false
              onClose()
            }
          }}
        >
          {t('取消')}
        </Button>
        {step === 'time' && (
          <Button disabled={busy} onClick={() => setStep('task')}>
            {t('上一步')}
          </Button>
        )}
        <Button
          variant="solid"
          disabled={busy}
          loading={busy}
          onClick={() => (step === 'task' ? setStep('time') : save.mutate())}
        >
          {t(step === 'task' ? '下一步' : '保存任务')}
        </Button>
      </footer>
      <Modal
        open={leave}
        title={t('有未保存的更改')}
        onOpenChange={(open) => {
          if (!open) {
            leaveResolve.current?.(false)
            setLeave(false)
          }
        }}
      >
        <div className="modal-footer">
          <Button
            onClick={() => {
              leaveResolve.current?.(false)
              setLeave(false)
            }}
          >
            {t('继续编辑')}
          </Button>
          <Button
            variant="solid"
            onClick={() => {
              leaveResolve.current?.(true)
              setLeave(false)
            }}
          >
            {t('放弃更改')}
          </Button>
        </div>
      </Modal>
      <Modal
        open={!!removing}
        onOpenChange={(open) => !open && setRemoving(null)}
        title={t('dag.delete')}
      >
        <div className="modal-body">
          {references.length ? (
            <div className="dag-reference-table" role="table">
              <div className="table-head" role="row">
                <span>{t('dag.referenceLocation')}</span>
                <span>{t('类型')}</span>
                <span>{t('操作')}</span>
              </div>
              {references.map((r, index) => (
                <div key={index} role="row">
                  <span>{r.name}</span>
                  <span>{r.kind}</span>
                  <IconButton
                    label={t('dag.goTo')}
                    onClick={() => {
                      setRemoving(null)
                      locate({
                        node_id: r.node_id,
                        path: r.node_id ? 'dag.nodes.' + r.node_id + '.' + r.field : 'dag.results',
                        message: '',
                      })
                    }}
                  >
                    <ArrowUpRight />
                  </IconButton>
                </div>
              ))}
            </div>
          ) : (
            <p>{nodeName(dag.nodes.find((n) => n.id === removing) ?? { name: '' })}</p>
          )}
        </div>
        <div className="modal-footer">
          <Button onClick={() => setRemoving(null)}>{t('取消')}</Button>
          <Button
            variant="solid"
            disabled={!!references.length || busy}
            onClick={() => {
              const nodes = dag.nodes.filter((n) => n.id !== removing)
              changeDag({ ...dag, nodes })
              setSelected(nodes[0]?.id ?? '')
              setExpanded(null)
              setRemoving(null)
            }}
          >
            {t('dag.delete')}
          </Button>
        </div>
      </Modal>
    </div>
  )
}
