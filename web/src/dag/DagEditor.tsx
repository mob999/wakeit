import { promptBlank } from './prompt'
import { ActionIcon } from '../actions/ActionIcon'
import { ErrorToast, useToast } from '../Toast'
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useBlocker } from '@tanstack/react-router'
import * as Menu from '@radix-ui/react-dropdown-menu'
import * as Popover from '@radix-ui/react-popover'
import {
  api,
  type Task,
  type TaskConfig,
  type Runner,
  type Agent,
  type DagIssue,
  type DagNode,
  type EditorLayout,
} from '../api'
import { t } from '../i18n'
import { Button, IconButton, Modal } from '../ui'
import { EditorMemoryContext, type EditorMemory } from '../EditorMemory'
import { PageBreadcrumbs } from '../PageHeader'
import { useSearchControls } from '../Search'
import { createTaskId, taskIdSuffix } from '../taskId'
import { ChoicePicker } from '../ChoicePicker'
import { DagGraph, type DagView } from './DagGraph'
import { DagCanvas, type CanvasHandle } from './DagCanvas'
import { CanvasControls } from './CanvasControls'
import { AgentPalette } from './AgentPalette'
import { NodeForm } from './NodeForm'
import { TimeFields } from './TimeFields'
import { DetailTabs } from '../DetailTabs'
import { MultiPicker } from './MultiPicker'
import { issues, outputs } from './model'
import { nodeName, nodeOptions, shortId } from './identity'
import { issueField, locateField, markFields } from './fields'
import { validDuration } from '../duration'
import { AgentLogo } from '../AgentLogo'
import { arrangeNodes, NODE_WIDTH, NODE_HEIGHT } from './layout'
import {
  GraphHistory,
  positionOf,
  connect,
  connectionIssue,
  createAgentNode,
  emptyLayout,
  removeNodes,
  type AgentChoice,
  type GraphDraft,
  type Point,
} from './workbench'
import './canvas.css'
function valueAt(config: TaskConfig, issue: DagIssue) {
  const base = issue.node_id ? config.dag?.nodes.find((n) => n.id === issue.node_id) : config
  return issueField(issue)
    .split('.')
    .reduce<any>((value, key) => value?.[key], base)
}
type Panel = { kind: 'node'; id: string } | { kind: 'outputs' } | null
type Picker = { source?: string; position?: Point } | null
type Deletion = { nodes: string[]; edges: { source: string; target: string }[] }
export function DagEditor({
  task,
  timezone,
  onClose,
  onSaved,
  embedded = false,
  active = true,
  onStateChange,
}: {
  task?: Task
  timezone: string
  onClose: () => void
  onSaved: (id: string) => void
  embedded?: boolean
  active?: boolean
  onStateChange?: (dirty: boolean, busy: boolean) => void
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
          execution: { timeout: null, start_grace: '1h', overlap: 'skip', misfire: 'latest' },
          dag: { nodes: [] },
        },
  )
  const [layout, setLayout] = useState<EditorLayout>(() =>
    structuredClone(task?.editor_layout ?? emptyLayout()),
  )
  const timeConfig = config
  const baseline = useRef({ config: JSON.stringify(config), layout: structuredClone(layout) })
  const initialIds = useRef(new Set(config.dag!.nodes.map((n) => n.id)))
  const [step, setStep] = useState<'task' | 'time'>('task'),
    [view, setView] = useState<DagView>('graph'),
    [fullscreen, setFullscreen] = useState(false),
    [search, setSearch] = useState('')
  const [panel, setPanel] = useState<Panel>(null),
    [selection, setSelection] = useState<string[]>([]),
    [picker, setPicker] = useState<Picker>(null)
  const [paletteOpen, setPaletteOpen] = useState(true),
    [panelSize, setPanelSize] = useState(520),
    [width, setWidth] = useState(1200)
  const [memory, setMemory] = useState<Record<string, EditorMemory>>({}),
    [serverErrors, setServerErrors] = useState<DagIssue[]>([]),
    [validated, setValidated] = useState(false),
    [requestError, setRequestError] = useState('')
  const [focus, setFocus] = useState<DagIssue | null>(null),
    [deletion, setDeletion] = useState<Deletion | null>(null),
    [arranging, setArranging] = useState(false)
  const [ghost, setGhost] = useState<{ choice: AgentChoice; point: Point } | null>(null),
    cancelDrag = useRef(false)
  const [leave, setLeave] = useState(false),
    leaveResolve = useRef<((value: boolean) => void) | null>(null)
  const editor = useRef<HTMLDivElement>(null),
    workbench = useRef<HTMLDivElement>(null),
    floatingBody = useRef<HTMLDivElement>(null),
    timePane = useRef<HTMLDivElement>(null),
    canvas = useRef<CanvasHandle>(null),
    nameField = useRef<HTMLInputElement>(null)
  const scrollPositions = useRef<Record<string, number>>({}),
    timeScroll = useRef(0),
    history = useRef(new GraphHistory()),
    [, historyTick] = useState(0),
    revision = useRef(0)
  const current = useRef({ config, layout })
  current.current = { config, layout }
  const busyRef = useRef(false),
    dirtyRef = useRef(false),
    menuTarget = useRef<DagIssue | null>(null),
    nextError = useRef(0),
    pickerFocus = useRef<DagIssue | null>(null)
  const timeValidation = useRef<() => DagIssue[]>(() => [])
  const toast = useToast(),
    client = useQueryClient(),
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
    node = panel?.kind === 'node' ? dag.nodes.find((n) => n.id === panel.id) : undefined
  const desktop = width >= 960,
    actualPalette = desktop && paletteOpen && view === 'graph'
  const maxPanelWidth = Math.max(360, Math.min(720, width - (actualPalette ? 200 : 0) - 352)),
    panelWidth = desktop ? Math.min(panelSize, maxPanelWidth) : 0
  const panelKey = panel?.kind === 'node' ? panel.id : (panel?.kind ?? '')
  const dirty =
    JSON.stringify(config) !== baseline.current.config ||
    JSON.stringify(layout) !== JSON.stringify(baseline.current.layout)
  dirtyRef.current = dirty
  useEffect(() => {
    if (step !== 'task' || view !== 'graph') setFullscreen(false)
  }, [step, view])
  useEffect(() => {
    const navigation = document.getElementById('primary-navigation')
    if (!fullscreen || !navigation) return
    const previous = navigation.inert
    navigation.inert = true
    return () => {
      navigation.inert = previous
    }
  }, [fullscreen])
  useLayoutEffect(() => {
    if (!workbench.current) return
    const ro = new ResizeObserver(([entry]) => {
      if (entry.contentRect.width) setWidth(entry.contentRect.width)
    })
    ro.observe(workbench.current)
    return () => ro.disconnect()
  }, [])
  useLayoutEffect(() => {
    if (floatingBody.current)
      floatingBody.current.scrollTop = scrollPositions.current[panelKey] ?? 0
  }, [panelKey, step])
  useLayoutEffect(() => {
    if (step === 'time' && timePane.current) timePane.current.scrollTop = timeScroll.current
  }, [step])
  const contexts = JSON.stringify(
    dag.nodes.map((n) => {
      const r = runners.data?.find((r) => r.id === n.runner || r.name === n.runner)
      return [n.id, n.runner, n.agent, n.cwd, r?.agents.find((a) => a.id === n.agent)?.digest]
    }),
  )
  useEffect(() => {
    const values: unknown[][] = JSON.parse(contexts)
    setMemory((previous) => {
      let changed = false
      const next = { ...previous }
      for (const [id, ...context] of values) {
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
  const requestLeave = useCallback(() => {
    if (busyRef.current) return Promise.resolve(false)
    if (!dirtyRef.current) return Promise.resolve(true)
    setLeave(true)
    return new Promise<boolean>((resolve) => {
      leaveResolve.current = resolve
    })
  }, [])
  useEffect(
    () => (embedded ? undefined : controls.registerGuard(requestLeave)),
    [controls.registerGuard, requestLeave, embedded],
  )
  useBlocker({
    shouldBlockFn: () => (embedded ? false : requestLeave().then((value) => !value)),
    enableBeforeUnload: () => !embedded && dirtyRef.current,
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
        (issue) =>
          JSON.stringify(valueAt(current.current.config, issue)) ===
          JSON.stringify(valueAt(next, issue)),
      ),
    )
    current.current = { ...current.current, config: next }
    setConfig(next)
  }
  function graph(): GraphDraft {
    return { dag: current.current.config.dag!, layout: current.current.layout }
  }
  function commit(next: GraphDraft, record = true) {
    if (record && history.current.record(graph(), next)) historyTick((n) => n + 1)
    revision.current++
    change({ ...current.current.config, dag: next.dag })
    current.current.layout = next.layout
    setLayout(next.layout)
  }
  function initialize(positions: Record<string, Point>) {
    const next = structuredClone(current.current.layout)
    for (const [id, point] of Object.entries(positions)) {
      if (initialIds.current.has(id) && !positionOf(baseline.current.layout, id))
        baseline.current.layout.positions[id] = point
      if (!positionOf(next, id) && current.current.config.dag!.nodes.some((n) => n.id === id))
        next.positions[id] = point
    }
    current.current.layout = next
    setLayout(next)
  }
  function travel(direction: 'undo' | 'redo') {
    if (busyRef.current) return
    const next = history.current[direction](graph())
    commit(next, false)
    historyTick((n) => n + 1)
    if (panel?.kind === 'node' && !next.dag.nodes.some((n) => n.id === panel.id)) setPanel(null)
    setSelection((old) => old.filter((id) => next.dag.nodes.some((n) => n.id === id)))
  }
  function openNode(id: string) {
    if (busyRef.current) return
    setSelection([id])
    setPanel({ kind: 'node', id })
  }
  function locateNode(id: string) {
    setSearch('')
    openNode(id)
    requestAnimationFrame(() => canvas.current?.locate(id))
  }
  function locate(issue: DagIssue) {
    if (issue.node_id || issue.path !== 'name')
      setStep(
        issue.path.startsWith('schedule') || issue.path.startsWith('execution.') ? 'time' : 'task',
      )
    if (issue.node_id) openNode(issue.node_id)
    else if (issue.path === 'dag.results') setPanel({ kind: 'outputs' })
    else if (issue.path === 'dag.nodes') setPicker({})
    setFocus(issue)
    if (issue.message)
      toast(issue.message + (issue.cycle?.length ? ' (' + issue.cycle.join(' → ') + ')' : ''))
  }
  function localErrors(): DagIssue[] {
    const result = issues(current.current.config.dag!)
    if (!current.current.config.name.trim())
      result.unshift({ path: 'name', message: t('请填写任务名称') })
    for (const n of current.current.config.dag!.nodes) {
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
  const structural = issues(dag).filter(
    (e) =>
      (e.path === 'dag.results' && dag.results != null) ||
      (e.node_id &&
        (issueField(e) === 'pre' ||
          (issueField(e) === 'prompt' &&
            !promptBlank(dag.nodes.find((n) => n.id === e.node_id)?.prompt)))),
  )
  const local = validated ? localErrors() : structural,
    errors = [...local, ...serverErrors.filter((e) => !local.some((l) => l.path === e.path))]
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
      const next = {
        ...current.current.config,
        id: task?.id ?? createTaskId(current.current.config.name, suffix),
      }
      const validation = await api<{ errors: DagIssue[] }>('/tasks/validate', 'POST', {
        config: next,
        editor_layout: current.current.layout,
      })
      setServerErrors(validation.errors)
      if (validation.errors.length) {
        locate(validation.errors[0])
        return null
      }
      return api<Task>('/tasks/' + encodeURIComponent(next.id), 'PUT', {
        config: next,
        editor_layout: current.current.layout,
        expected_revision: task?.revision ?? null,
      })
    },
    onSuccess: (result) => {
      if (!result) return
      dirtyRef.current = false
      busyRef.current = false
      baseline.current = {
        config: JSON.stringify(current.current.config),
        layout: structuredClone(current.current.layout),
      }
      client.invalidateQueries({ queryKey: ['tasks'] })
      onSaved(result.id)
    },
    onError: (error: Error) => setRequestError(error.message),
  })
  const busy = save.isPending || arranging
  useEffect(() => {
    onStateChange?.(dirty, busy)
  }, [dirty, busy, onStateChange])
  busyRef.current = busy
  useLayoutEffect(() => {
    markFields(
      editor.current,
      errors.filter((e) => !e.node_id),
    )
    if (busy || !focus) return
    if (!focus.node_id && focus.path === 'name') nameField.current?.focus()
    else locateField(editor.current, focus)
    setFocus(null)
  })
  function add(choice: AgentChoice, sourceId?: string, position?: Point) {
    if (busyRef.current) return
    const runner = runners.data?.find((r) => r.id === choice.runner.id && !r.revoked),
      agent = runner?.agents.find((a) => a.id === choice.agent.id && a.available)
    if (!runner || !agent) {
      toast(t('dag.agentUnavailable'))
      return
    }
    const before = graph(),
      source = before.dag.nodes.find((n) => n.id === sourceId),
      next = createAgentNode(before.dag.nodes, { runner, agent }, source)
    let p = position ?? canvas.current?.center() ?? { x: 40, y: 40 }
    if (!position || sourceId) {
      while (
        Object.values(before.layout.positions).some(
          (existing) =>
            p.x < existing.x + NODE_WIDTH + 24 &&
            p.x + NODE_WIDTH + 24 > existing.x &&
            p.y < existing.y + NODE_HEIGHT + 24 &&
            p.y + NODE_HEIGHT + 24 > existing.y,
        )
      )
        p = { ...p, y: p.y + NODE_HEIGHT + 32 }
    }
    commit({
      dag: { ...before.dag, nodes: [...before.dag.nodes, next] },
      layout: { version: 1, positions: { ...before.layout.positions, [next.id]: p } },
    })
    setPicker(null)
    setGhost(null)
    openNode(next.id)
    const target = { node_id: next.id, path: 'dag.nodes.' + next.id + '.name', message: '' }
    if (picker) pickerFocus.current = target
    else setFocus(target)
  }
  function copyNode(source: DagNode) {
    const before = graph(),
      copy = structuredClone(source)
    copy.id = 'node_' + crypto.randomUUID().replaceAll('-', '').slice(0, 10)
    copy.name += ' ' + t('dag.copySuffix')
    const position = positionOf(before.layout, source.id) ?? { x: 40, y: 40 }
    commit({
      dag: { ...before.dag, nodes: [...before.dag.nodes, copy] },
      layout: {
        version: 1,
        positions: {
          ...before.layout.positions,
          [copy.id]: { x: position.x + 40, y: position.y + 128 },
        },
      },
    })
    openNode(copy.id)
    menuTarget.current = { node_id: copy.id, path: 'dag.nodes.' + copy.id + '.name', message: '' }
  }
  function connection(
    source: string,
    target: string,
    previous?: { source: string; target: string },
  ) {
    if (busyRef.current) return
    const before = graph(),
      error = connectionIssue(before.dag, source, target, previous)
    if (error) {
      toast(t('dag.connection.' + error))
      return
    }
    commit({ ...before, dag: connect(before.dag, source, target, previous) })
  }
  function resultingDeletion(value: Deletion) {
    const next = removeNodes(graph(), value.nodes)
    next.dag.nodes = next.dag.nodes.map((n) => ({
      ...n,
      pre: n.pre.filter(
        (source) => !value.edges.some((e) => e.source === source && e.target === n.id),
      ),
    }))
    return next
  }
  function impacts(value: Deletion) {
    const next = resultingDeletion(value),
      before = issues(graph().dag)
    return issues(next.dag).filter(
      (e) =>
        ((e.path === 'dag.results' && graph().dag.results != null) ||
          (e.node_id &&
            issueField(e) === 'prompt' &&
            !promptBlank(next.dag.nodes.find((n) => n.id === e.node_id)?.prompt))) &&
        !before.some((p) => p.path === e.path && p.message === e.message),
    )
  }
  function finishDelete(value: Deletion) {
    commit(resultingDeletion(value))
    setDeletion(null)
    setSelection((old) => old.filter((id) => !value.nodes.includes(id)))
    if (panel?.kind === 'node' && value.nodes.includes(panel.id)) setPanel(null)
  }
  function requestDelete(nodes: string[], edges: { source: string; target: string }[] = []) {
    if (busyRef.current) return
    const value = { nodes, edges }
    if (impacts(value).length) setDeletion(value)
    else finishDelete(value)
  }
  async function organize() {
    if (busyRef.current || !dag.nodes.length) return
    const before = graph(),
      generation = revision.current
    setArranging(true)
    try {
      const positions = await arrangeNodes(before.dag.nodes)
      if (generation === revision.current) {
        commit({ ...graph(), layout: { version: 1, positions } })
        requestAnimationFrame(() => canvas.current?.fit())
      }
    } catch {
      toast(t('dag.layoutFailed'))
    } finally {
      setArranging(false)
    }
  }
  function paletteDrag(choice: AgentChoice, point: Point, phase: 'move' | 'drop' | 'cancel') {
    if (phase === 'cancel') {
      setGhost(null)
      cancelDrag.current = false
      return
    }
    if (cancelDrag.current) {
      if (phase === 'drop') cancelDrag.current = false
      return
    }
    if (phase === 'move') {
      if (!busyRef.current) setGhost({ choice, point })
      return
    }
    setGhost(null)
    const target = document.elementFromPoint(point.x, point.y)
    if (
      view === 'graph' &&
      target?.closest('.dag-interactive-canvas') &&
      !target.closest('.dag-floating-panel, .react-flow__node, .react-flow__edge')
    )
      add(choice, undefined, canvas.current?.point(point))
  }
  const handlers = useRef({ travel, setPanel, ghost, picker, panel, fullscreen })
  handlers.current = { travel, setPanel, ghost, picker, panel, fullscreen }
  useEffect(() => {
    function keyboard(event: KeyboardEvent) {
      if (event.defaultPrevented || busyRef.current || step !== 'task' || !active) return
      const target = event.target instanceof Element ? event.target : null
      const overlay = document.querySelector(
        '[data-radix-popper-content-wrapper], [role=dialog][data-state=open]:not(.creation-modal)',
      )
      if (event.key === 'Escape') {
        if (overlay) return
        if (handlers.current.ghost) {
          cancelDrag.current = true
          setGhost(null)
        } else if (handlers.current.picker) setPicker(null)
        else if (handlers.current.panel) handlers.current.setPanel(null)
        else if (handlers.current.fullscreen) setFullscreen(false)
        event.preventDefault()
      }
      if (
        (event.metaKey || event.ctrlKey) &&
        event.key.toLowerCase() === 'z' &&
        !target?.closest('input,textarea,[contenteditable=true]') &&
        !overlay
      ) {
        event.preventDefault()
        handlers.current.travel(event.shiftKey ? 'redo' : 'undo')
      }
    }
    window.addEventListener('keydown', keyboard, embedded)
    return () => window.removeEventListener('keydown', keyboard, embedded)
  }, [step, active, embedded])
  function nodeActions(n: DagNode) {
    return (
      <Menu.Root>
        <Menu.Trigger asChild>
          <IconButton label={t('dag.nodeActions')} disabled={busy}>
            <ActionIcon name="more" />
          </IconButton>
        </Menu.Trigger>
        <Menu.Portal>
          <Menu.Content
            className="dropdown"
            align="end"
            onCloseAutoFocus={(e) => {
              if (menuTarget.current) {
                e.preventDefault()
                const target = menuTarget.current
                menuTarget.current = null
                setFocus(target)
              }
            }}
          >
            <Menu.Item
              disabled={busy}
              onSelect={() =>
                setPicker({
                  source: n.id,
                  position: {
                    x: (positionOf(layout, n.id)?.x ?? 40) + NODE_WIDTH + 72,
                    y: positionOf(layout, n.id)?.y ?? 40,
                  },
                })
              }
            >
              {t('dag.addNext')}
            </Menu.Item>
            <Menu.Item disabled={busy} onSelect={() => copyNode(n)}>
              {t('dag.duplicate')}
            </Menu.Item>
            <Menu.Item
              disabled={busy}
              onSelect={() => requestDelete(selection.includes(n.id) ? selection : [n.id])}
            >
              {t('dag.delete')}
            </Menu.Item>
          </Menu.Content>
        </Menu.Portal>
      </Menu.Root>
    )
  }
  const outputNodes = dag.results ?? outputs(dag)
  const deletionIssues = deletion ? impacts(deletion) : []
  return (
    <div
      className={
        'dag-editor dag-canvas-editor' +
        (embedded ? ' creation-dag-form' : '') +
        (fullscreen ? ' is-fullscreen' : '')
      }
      ref={editor}
    >
      {!embedded && (
        <PageBreadcrumbs
          label={t('Tasks')}
          crumbs={[
            {
              label: config.name || t('dag.new'),
              content: (
                <input
                  className="dag-breadcrumb-title"
                  ref={nameField}
                  aria-label={t('任务名称')}
                  aria-invalid={errors.some((e) => !e.node_id && e.path === 'name')}
                  placeholder={t('dag.new')}
                  autoFocus={!task}
                  disabled={busy}
                  value={config.name}
                  onChange={(e) => change({ ...config, name: e.target.value })}
                />
              ),
            },
          ]}
          onBack={onClose}
        />
      )}
      {embedded && (
        <div className="creation-dag-name">
          <label htmlFor="creation-dag-name">{t('任务名称')}</label>
          <input
            id="creation-dag-name"
            ref={nameField}
            value={config.name}
            disabled={busy}
            onChange={(event) => change({ ...config, name: event.target.value })}
          />
        </div>
      )}
      <div className="dag-stepbar">
        {embedded ? (
          <DetailTabs
            idPrefix="creation-dag-section"
            className="creation-section-tabs"
            label={t('任务编辑')}
            value={step}
            disabled={busy}
            tabs={[
              { key: 'task', label: t('creation.structure') },
              { key: 'time', label: t('creation.schedule') },
            ]}
            onChange={setStep}
          />
        ) : (
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
                {t(value === 'task' ? 'creation.structure' : 'creation.schedule')}
              </button>
            ))}
          </div>
        )}
        <div className="dag-toolbar" hidden={step !== 'task'}>
          <div className="dag-toolbar-actions">
            <div className="action-group" role="group" aria-label={t('actions.view')}>
              {desktop && view === 'graph' && (
                <IconButton
                  label={t('dag.toggleAgents')}
                  aria-pressed={paletteOpen}
                  disabled={busy}
                  onClick={() => setPaletteOpen((v) => !v)}
                >
                  <ActionIcon name="sidebar" />
                </IconButton>
              )}
              <IconButton
                label={t('dag.graph')}
                aria-pressed={view === 'graph'}
                disabled={busy}
                onClick={() => setView('graph')}
              >
                <ActionIcon name="graph" />
              </IconButton>
              <IconButton
                label={t('dag.list')}
                aria-pressed={view === 'list'}
                disabled={busy}
                onClick={() => setView('list')}
              >
                <ActionIcon name="list" />
              </IconButton>
            </div>
            <div className="action-group" role="group" aria-label={t('actions.construction')}>
              <IconButton label={t('dag.addNode')} disabled={busy} onClick={() => setPicker({})}>
                <ActionIcon name="add" />
              </IconButton>
              <Popover.Root>
                <Popover.Trigger asChild>
                  <IconButton label={t('dag.search')} disabled={busy}>
                    <ActionIcon name="search" />
                  </IconButton>
                </Popover.Trigger>
                <Popover.Portal>
                  <Popover.Content className="dag-search-popover" align="end" sideOffset={6}>
                    <input
                      aria-label={t('dag.search')}
                      placeholder={t('dag.search')}
                      disabled={busy}
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
                              <button disabled={busy} onClick={() => locateNode(n.id)}>
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
            </div>
            <div className="action-group" role="group" aria-label={t('actions.editHistory')}>
              <IconButton
                label={t('dag.undo')}
                disabled={busy || !history.current.past.length}
                onClick={() => travel('undo')}
              >
                <ActionIcon name="undo" />
              </IconButton>
              <IconButton
                label={t('dag.redo')}
                disabled={busy || !history.current.future.length}
                onClick={() => travel('redo')}
              >
                <ActionIcon name="redo" />
              </IconButton>
            </div>
            <div className="action-group" role="group" aria-label={t('actions.graphSettings')}>
              <IconButton
                label={t('dag.organize')}
                disabled={busy || !dag.nodes.length}
                onClick={organize}
              >
                <ActionIcon name="organize" />
              </IconButton>
              <IconButton
                label={t('dag.outputSettings')}
                aria-pressed={panel?.kind === 'outputs'}
                disabled={busy}
                onClick={() => setPanel({ kind: 'outputs' })}
              >
                <ActionIcon name="outputs" />
              </IconButton>
            </div>
          </div>
        </div>
      </div>
      <div className="dag-canvas-body" ref={workbench}>
        <div
          className="dag-task-stage"
          hidden={step !== 'task'}
          inert={step !== 'task'}
          id={embedded ? 'creation-dag-section-task-panel' : undefined}
          role={embedded ? 'tabpanel' : undefined}
          aria-labelledby={embedded ? 'creation-dag-section-task-tab' : undefined}
        >
          {actualPalette && (
            <aside className="dag-palette-dock" aria-label={t('Agents')}>
              <header>Agents</header>
              <AgentPalette
                runners={runners.data ?? []}
                disabled={busy}
                onChoose={(choice) => add(choice)}
                onDrag={paletteDrag}
              />
            </aside>
          )}
          <div className="dag-scene">
            <div className="dag-canvas-mount" hidden={view !== 'graph'}>
              <DagCanvas
                ref={canvas}
                nodes={dag.nodes}
                layout={layout}
                runners={runners.data ?? []}
                agents={agents}
                selected={selection}
                outputs={outputs(dag)}
                invalid={errors.flatMap((e) => (e.node_id ? [e.node_id] : []))}
                search={search}
                disabled={busy}
                active={step === 'task' && view === 'graph'}
                panelWidth={panel && desktop ? panelWidth : 0}
                actions={nodeActions}
                onSelect={(ids) =>
                  setSelection((old) => (JSON.stringify(old) === JSON.stringify(ids) ? old : ids))
                }
                onOpen={openNode}
                onBlank={() => {
                  setPanel(null)
                  setSelection([])
                }}
                onPositions={(positions) => {
                  if (!busyRef.current)
                    commit({
                      ...graph(),
                      layout: {
                        version: 1,
                        positions: { ...graph().layout.positions, ...positions },
                      },
                    })
                }}
                onInitialize={initialize}
                valid={(source, target, previous) =>
                  !connectionIssue(dag, source, target, previous)
                }
                onConnect={connection}
                onDelete={requestDelete}
                onAdd={(source, position) => setPicker({ source, position })}
                onLayoutError={() => toast(t('dag.layoutFailed'))}
              />
            </div>
            <div className="dag-table-stage" hidden={view !== 'list'}>
              <section className="content-card">
                <DagGraph
                  nodes={dag.nodes}
                  runners={runners.data}
                  agents={agents}
                  selected={selection[0]}
                  onSelect={openNode}
                  view="list"
                  search={search}
                  hideHeader
                  disabled={busy}
                  invalid={errors.flatMap((e) => (e.node_id ? [e.node_id] : []))}
                  outputIds={outputs(dag)}
                  rowActions={nodeActions}
                />
              </section>
            </div>
            <CanvasControls
              fullscreen={fullscreen}
              disabled={busy}
              hidden={view !== 'graph' || (!desktop && !!panel)}
              style={{ right: panel && desktop ? panelWidth + 32 : 16 }}
              onZoom={(direction) => canvas.current?.zoom(direction * 0.15)}
              onToggleFullscreen={() => setFullscreen((value) => !value)}
            />
          </div>
          {panel && (
            <section
              className="dag-floating-panel content-card"
              aria-label={node ? t('dag.nodeConfig') : t('dag.outputSettings')}
              style={desktop ? { width: panelWidth } : undefined}
            >
              {desktop && (
                <div
                  className="resize-handle dag-panel-resizer"
                  role="separator"
                  aria-label={t('dag.resizePanel')}
                  aria-orientation="vertical"
                  aria-valuenow={panelWidth}
                  aria-valuemin={360}
                  aria-valuemax={maxPanelWidth}
                  tabIndex={busy ? -1 : 0}
                  onPointerDown={(e) => {
                    if (busy || e.button !== 0) return
                    e.preventDefault()
                    e.currentTarget.setPointerCapture(e.pointerId)
                  }}
                  onPointerMove={(e) => {
                    if (e.currentTarget.hasPointerCapture(e.pointerId))
                      setPanelSize(
                        Math.max(
                          360,
                          Math.min(
                            maxPanelWidth,
                            e.currentTarget.parentElement!.getBoundingClientRect().right -
                              e.clientX,
                          ),
                        ),
                      )
                  }}
                  onPointerUp={(e) => {
                    if (e.currentTarget.hasPointerCapture(e.pointerId))
                      e.currentTarget.releasePointerCapture(e.pointerId)
                  }}
                  onDoubleClick={() => setPanelSize(520)}
                  onKeyDown={(e) => {
                    const delta = e.key === 'ArrowLeft' ? 10 : e.key === 'ArrowRight' ? -10 : 0
                    if (delta) {
                      e.preventDefault()
                      setPanelSize(Math.max(360, Math.min(maxPanelWidth, panelWidth + delta)))
                    }
                  }}
                />
              )}
              <div className="dag-node-context dag-floating-header">
                {node ? (
                  <>
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
                        onChange={openNode}
                      />
                    ) : (
                      <strong>{nodeName(node)}</strong>
                    )}
                    {nodeActions(node)}
                  </>
                ) : (
                  <strong>{t('dag.outputSettings')}</strong>
                )}
                <IconButton label={t('关闭')} disabled={busy} onClick={() => setPanel(null)}>
                  <ActionIcon name="close" />
                </IconButton>
              </div>
              <div
                className="dag-floating-body"
                ref={floatingBody}
                onScroll={(e) => {
                  scrollPositions.current[panelKey] = e.currentTarget.scrollTop
                }}
              >
                {node && (
                  <EditorMemoryContext.Provider
                    value={{
                      active: step === 'task',
                      value: memory[node.id] ?? {},
                      update: (update) =>
                        setMemory((previous) => ({
                          ...previous,
                          [node.id]: {
                            ...(typeof update === 'function'
                              ? update(previous[node.id] ?? {})
                              : update),
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
                      focusTarget={workbench.current}
                      onChange={(next) => {
                        const before = graph()
                        const updated = {
                          ...before.dag,
                          nodes: before.dag.nodes.map((n) => (n.id === next.id ? next : n)),
                        }
                        if (JSON.stringify(node.pre) !== JSON.stringify(next.pre))
                          commit({ ...before, dag: updated })
                        else change({ ...current.current.config, dag: updated })
                      }}
                    />
                  </EditorMemoryContext.Provider>
                )}
                {panel.kind === 'outputs' && (
                  <div className="dag-output-settings" data-field="dag.results">
                    <div className="field">
                      <label>{t('dag.resultsMode')}</label>
                      <ChoicePicker
                        label={t('dag.resultsMode')}
                        disabled={busy}
                        value={dag.results == null ? 'auto' : 'custom'}
                        options={[
                          { value: 'auto', label: t('dag.autoResults') },
                          { value: 'custom', label: t('dag.customResults') },
                        ]}
                        onChange={(value) =>
                          commit({
                            ...graph(),
                            dag: {
                              ...dag,
                              results: value === 'auto' ? undefined : [...outputs(dag)],
                            },
                          })
                        }
                      />
                    </div>
                    {dag.results != null && (
                      <div className="field">
                        <label>{t('dag.results')}</label>
                        <MultiPicker
                          label={t('dag.results')}
                          disabled={busy}
                          chips
                          value={dag.results}
                          options={[
                            ...nodeOptions(dag.nodes),
                            ...dag.results
                              .filter((id) => !dag.nodes.some((n) => n.id === id))
                              .map((id) => ({ id, name: id, description: t('dag.missingOutput') })),
                          ]}
                          onChange={(results) => commit({ ...graph(), dag: { ...dag, results } })}
                        />
                      </div>
                    )}
                    <div className="dag-output-rows">
                      {outputNodes.map((id) => {
                        const n = dag.nodes.find((n) => n.id === id)
                        return (
                          <div key={id}>
                            <span>{n ? nodeName(n) : id}</span>
                            <span className="quiet text-muted mono">
                              {n ? shortId(n.id) : t('dag.missingOutput')}
                            </span>
                            <IconButton
                              label={t('dag.locateSource', { name: n ? nodeName(n) : id })}
                              disabled={busy || !n}
                              onClick={() => locateNode(id)}
                            >
                              <ActionIcon name="open" />
                            </IconButton>
                          </div>
                        )
                      })}
                    </div>
                  </div>
                )}
              </div>
            </section>
          )}
          {runners.error && <ErrorToast error={runners.error} />}
        </div>
        <div
          className={
            'dag-editor-scroll dag-time-scroll' + (embedded ? ' creation-schedule-scroll' : '')
          }
          id={embedded ? 'creation-dag-section-time-panel' : undefined}
          role={embedded ? 'tabpanel' : undefined}
          aria-labelledby={embedded ? 'creation-dag-section-time-tab' : undefined}
          hidden={step !== 'time'}
          inert={step !== 'time'}
          ref={timePane}
          onScroll={(e) => {
            timeScroll.current = e.currentTarget.scrollTop
          }}
        >
          <TimeFields
            config={timeConfig}
            timezone={timezone}
            disabled={busy}
            active={step === 'time'}
            onChange={change}
            validation={timeValidation}
          />
        </div>
      </div>
      <footer className="dag-editor-footer">
        {requestError && <ErrorToast error={requestError} />}
        {errors.length > 0 && (
          <Button
            disabled={busy}
            onClick={() => locate(errors[nextError.current++ % errors.length])}
          >
            {t('dag.nextIssue')}
          </Button>
        )}
        <Button
          disabled={busy}
          onClick={async () => {
            if (embedded) {
              onClose()
              return
            }
            if (await requestLeave()) {
              dirtyRef.current = false
              onClose()
            }
          }}
        >
          {t('取消')}
        </Button>
        {!embedded && step === 'time' && (
          <Button disabled={busy} onClick={() => setStep('task')}>
            {t('上一步')}
          </Button>
        )}
        <Button
          variant="solid"
          disabled={busy}
          loading={busy}
          onClick={() => (!embedded && step === 'task' ? setStep('time') : save.mutate())}
        >
          {t(embedded ? 'creation.create' : step === 'task' ? '下一步' : '保存任务')}
        </Button>
      </footer>
      <Modal
        open={!!picker}
        title={t(picker?.source ? 'dag.addNext' : 'dag.addNode')}
        onCloseAutoFocus={(event) => {
          if (pickerFocus.current) {
            event.preventDefault()
            const target = pickerFocus.current
            pickerFocus.current = null
            requestAnimationFrame(() => setFocus(target))
          }
        }}
        onOpenChange={(open) => {
          if (!open) setPicker(null)
        }}
      >
        <AgentPalette
          compact
          runners={runners.data ?? []}
          disabled={busy}
          onChoose={(choice) => add(choice, picker?.source, picker?.position)}
          onDrag={(choice, point, phase) => {
            if (phase === 'drop') add(choice, picker?.source, picker?.position)
            else if (phase === 'cancel') setGhost(null)
          }}
        />
      </Modal>
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
        open={!!deletion}
        title={t('dag.delete')}
        onOpenChange={(open) => {
          if (!open) setDeletion(null)
        }}
      >
        <div className="modal-body">
          <div className="dag-reference-table" role="table">
            <div className="table-head" role="row">
              <span>{t('dag.referenceLocation')}</span>
              <span>{t('类型')}</span>
              <span>{t('操作')}</span>
            </div>
            {deletionIssues.map((issue, index) => (
              <div role="row" key={index}>
                <span>
                  {issue.node_id
                    ? nodeName(
                        dag.nodes.find((n) => n.id === issue.node_id) ?? { name: issue.node_id },
                      )
                    : t('dag.results')}
                </span>
                <span>{issue.node_id ? t('指令') : t('dag.outputNode')}</span>
                <IconButton
                  label={t('dag.goTo')}
                  onClick={() => {
                    setDeletion(null)
                    locate(issue)
                  }}
                >
                  <ActionIcon name="open" />
                </IconButton>
              </div>
            ))}
          </div>
        </div>
        <div className="modal-footer">
          <Button onClick={() => setDeletion(null)}>{t('取消')}</Button>
          <Button
            variant="solid"
            disabled={busy}
            onClick={() => deletion && finishDelete(deletion)}
          >
            {t('dag.delete')}
          </Button>
        </div>
      </Modal>
      {ghost &&
        createPortal(
          <div
            className="dag-agent-ghost"
            aria-hidden="true"
            style={{ left: ghost.point.x - NODE_WIDTH / 2, top: ghost.point.y - NODE_HEIGHT / 2 }}
          >
            <AgentLogo name={ghost.choice.agent.name} logo={ghost.choice.agent.logo} />
            <strong>{ghost.choice.agent.name}</strong>
            <span>{ghost.choice.runner.name}</span>
          </div>,
          document.body,
        )}
    </div>
  )
}
