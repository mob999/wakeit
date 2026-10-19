import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useBlocker } from '@tanstack/react-router'
import * as Menu from '@radix-ui/react-dropdown-menu'
import { Plus, MoreHorizontal, ChevronLeft } from 'lucide-react'
import { api, type Task, type TaskConfig, type Runner, type Agent, type DagIssue } from '../api'
import { t } from '../i18n'
import { Button, IconButton, Field, Modal } from '../ui'
import { PageBreadcrumbs } from '../PageHeader'
import { PaneResizeHandle } from '../PaneResizeHandle'
import { useSearchControls } from '../Search'
import { createTaskId, taskIdSuffix } from '../taskId'
import { DagGraph } from './DagGraph'
import { NodeForm } from './NodeForm'
import { TimeFields } from './TimeFields'
import { MultiPicker } from './MultiPicker'
import { newNode, issues, referencesTo, outputs } from './model'

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
  const baseline = useRef(JSON.stringify(config))
  const [selected, setSelected] = useState(config.dag!.nodes[0]?.id ?? '')
  const [step, setStep] = useState<'task' | 'time'>('task')
  const [mobile, setMobile] = useState<'node' | 'list'>(task ? 'list' : 'node')
  const [width, setWidth] = useState(400)
  const [room, setRoom] = useState(0)
  const pane = useRef<HTMLDivElement>(null)
  const [errors, setErrors] = useState<DagIssue[]>([])
  const [error, setError] = useState('')
  const [validating, setValidating] = useState(false)
  const [removing, setRemoving] = useState(false)
  const [leave, setLeave] = useState(false)
  const leaveResolve = useRef<((value: boolean) => void) | null>(null)
  const client = useQueryClient(),
    controls = useSearchControls()
  const runners = useQuery<Runner[]>({
    queryKey: ['runners'],
    queryFn: () => api('/runners'),
    refetchInterval: 3000,
  })
  const agents = (runners.data ?? []).flatMap((r) =>
    r.agents.map((a) => ({ ...a, runner: r.name, runner_id: r.id })),
  ) as Agent[]
  const dirty = JSON.stringify(config) !== baseline.current
  const dirtyRef = useRef(dirty)
  dirtyRef.current = dirty
  const busyRef = useRef(false)
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
    shouldBlockFn: () => requestLeave().then((v) => !v),
    enableBeforeUnload: () => dirtyRef.current,
  })
  useEffect(
    () => () => {
      leaveResolve.current?.(false)
    },
    [],
  )
  useEffect(() => {
    if (!pane.current) return
    const observer = new ResizeObserver(([entry]) => setRoom(entry.contentRect.width))
    observer.observe(pane.current)
    return () => observer.disconnect()
  }, [step])
  const dag = config.dag!,
    node = dag.nodes.find((n) => n.id === selected)
  const changeDag = (next: typeof dag) => setConfig((c) => ({ ...c, dag: next }))
  const locate = (issue: DagIssue) => {
    setStep('task')
    if (issue.node_id) {
      setSelected(issue.node_id)
      setMobile('node')
    }
    requestAnimationFrame(() =>
      pane.current?.querySelector<HTMLElement>('[aria-invalid=true]')?.focus(),
    )
  }
  const validate = async () => {
    const local = issues(dag)
    if (!config.name.trim()) local.unshift({ path: 'name', message: t('请填写任务名称') })
    if (local.length) {
      setErrors(local)
      locate(local[0])
      return false
    }
    setValidating(true)
    try {
      const result = await api<{ errors: DagIssue[] }>('/tasks/validate', 'POST', {
        config: { ...config, id: task?.id ?? createTaskId(config.name, suffix) },
      })
      setErrors(result.errors)
      if (result.errors.length) {
        locate(result.errors[0])
        return false
      }
      return true
    } catch (e) {
      setError((e as Error).message)
      return false
    } finally {
      setValidating(false)
    }
  }
  const next = async () => {
    if (busyRef.current) return
    setError('')
    if (await validate()) setStep('time')
  }
  const save = useMutation({
    mutationFn: async () => {
      setError('')
      if (!(await validate())) throw new Error(t('dag.fixErrors'))
      if (config.schedule) await api('/schedule/preview', 'POST', config.schedule)
      const id = task?.id ?? createTaskId(config.name, suffix)
      return api<Task>(`/tasks/${encodeURIComponent(id)}`, 'PUT', {
        config: { ...config, id },
        expected_revision: task?.revision ?? null,
      })
    },
    onSuccess: (result) => {
      baseline.current = JSON.stringify(config)
      dirtyRef.current = false
      busyRef.current = false
      client.invalidateQueries({ queryKey: ['tasks'] })
      onSaved(result.id)
    },
    onError: (e) => setError(e.message),
  })
  const busy = save.isPending || validating
  busyRef.current = busy
  const used = node ? referencesTo(dag, node.id) : []
  const resultUsed = !!node && !!dag.results?.includes(node.id)
  const blockedDelete = used.length > 0 || resultUsed
  const invalid = errors.flatMap((e) => (e.node_id ? [e.node_id] : []))
  return (
    <div className="dag-editor">
      <PageBreadcrumbs
        label={t('Tasks')}
        crumbs={[{ label: task ? task.config.name : t('dag.new') }]}
        onBack={onClose}
      />
      <div className="tabs" role="tablist" aria-label={t('任务编辑')}>
        <button
          role="tab"
          aria-selected={step === 'task'}
          className={step === 'task' ? 'active' : ''}
          disabled={busy}
          onClick={() => setStep('task')}
        >
          {t('任务')}
        </button>
        <button
          role="tab"
          aria-selected={step === 'time'}
          className={step === 'time' ? 'active' : ''}
          disabled={busy}
          onClick={() => void next()}
        >
          {t('时间')}
        </button>
      </div>
      {!!errors.length && (
        <div className="dag-error-list" role="alert">
          {errors.map((e, i) => (
            <button key={i} onClick={() => locate(e)}>
              {e.node_id
                ? `${dag.nodes.find((n) => n.id === e.node_id)?.name || t('dag.unnamed')}: `
                : ''}
              {e.message}
              {!!e.cycle?.length && ` (${e.cycle.join(' → ')})`}
            </button>
          ))}
        </div>
      )}
      {step === 'task' ? (
        <>
          <div className="dag-editor-name">
            <Field label={t('任务名称')}>
              <input
                disabled={busy}
                value={config.name}
                aria-invalid={errors.some((e) => e.path === 'name')}
                onChange={(e) => setConfig((c) => ({ ...c, name: e.target.value }))}
              />
            </Field>
            <Field label={t('dag.results')}>
              <MultiPicker
                label={t('dag.results')}
                disabled={busy}
                value={dag.results ?? []}
                empty={t('dag.autoResults')}
                options={[
                  { id: '__auto', name: t('dag.autoResults') },
                  ...dag.nodes.map((n) => ({ id: n.id, name: n.name || t('dag.unnamed') })),
                ]}
                onChange={(ids) =>
                  changeDag({ ...dag, results: ids.includes('__auto') ? undefined : ids })
                }
              />
            </Field>
          </div>
          <div
            className="dag-editor-workspace"
            ref={pane}
            data-mobile-panel={mobile}
            style={{ gridTemplateColumns: `minmax(0,1fr) ${width}px` }}
          >
            <DagGraph
              nodes={dag.nodes}
              agents={agents}
              selected={selected}
              onSelect={(id) => {
                setSelected(id)
                setMobile('node')
              }}
              invalid={invalid}
              actions={
                <IconButton
                  label={t('dag.addNode')}
                  disabled={busy}
                  onClick={() => {
                    const next = newNode()
                    changeDag({ ...dag, nodes: [...dag.nodes, next] })
                    setSelected(next.id)
                    setMobile('node')
                  }}
                >
                  <Plus />
                </IconButton>
              }
            />
            <PaneResizeHandle
              label={t('dag.resize')}
              width={Math.max(0, room - width)}
              min={Math.max(0, room - 600)}
              max={Math.max(0, room - 320)}
              onResize={(left) => setWidth(room - left)}
              onReset={() => setWidth(400)}
              style={{ right: width - 4, left: 'auto' }}
            />
            <section className="dag-node-config">
              <header className="dag-config-head">
                <span>
                  <IconButton
                    className="dag-mobile-back"
                    label={t('dag.nodes')}
                    disabled={busy}
                    onClick={() => setMobile('list')}
                  >
                    <ChevronLeft />
                  </IconButton>
                  {t('dag.nodeConfig')}
                </span>
                {node && (
                  <Menu.Root>
                    <Menu.Trigger asChild>
                      <IconButton label={t('dag.nodeActions')} disabled={busy}>
                        <MoreHorizontal />
                      </IconButton>
                    </Menu.Trigger>
                    <Menu.Portal>
                      <Menu.Content className="dropdown" align="end">
                        <Menu.Item
                          onSelect={() => {
                            const copy = {
                              ...structuredClone(node),
                              id: newNode().id,
                              name: `${node.name} ${t('dag.copySuffix')}`,
                            }
                            changeDag({ ...dag, nodes: [...dag.nodes, copy] })
                            setSelected(copy.id)
                          }}
                        >
                          {t('dag.duplicate')}
                        </Menu.Item>
                        <Menu.Item onSelect={() => setRemoving(true)}>{t('dag.delete')}</Menu.Item>
                      </Menu.Content>
                    </Menu.Portal>
                  </Menu.Root>
                )}
              </header>
              {node ? (
                <NodeForm
                  key={node.id}
                  node={node}
                  dag={dag}
                  runners={runners.data ?? []}
                  disabled={busy}
                  errors={errors}
                  onChange={(next) =>
                    changeDag({
                      ...dag,
                      nodes: dag.nodes.map((n) => (n.id === next.id ? next : n)),
                    })
                  }
                />
              ) : (
                <p className="quiet padded">{t('dag.noNodes')}</p>
              )}
              {runners.error && (
                <p className="error padded" role="alert">
                  {runners.error.message}
                  <Button onClick={() => void runners.refetch()}>{t('重试')}</Button>
                </p>
              )}
            </section>
          </div>
        </>
      ) : (
        <TimeFields config={config} timezone={timezone} disabled={busy} onChange={setConfig} />
      )}
      <footer className="dag-editor-footer">
        {error && (
          <span className="error" role="alert">
            {error}
          </span>
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
          onClick={() => (step === 'task' ? void next() : save.mutate())}
        >
          {step === 'task' ? t('下一步') : t('保存任务')}
        </Button>
      </footer>
      <Modal
        open={leave}
        onOpenChange={(v) => {
          if (!v) {
            leaveResolve.current?.(false)
            setLeave(false)
          }
        }}
        title={t('有未保存的更改')}
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
              dirtyRef.current = false
              leaveResolve.current?.(true)
              setLeave(false)
            }}
          >
            {t('放弃更改')}
          </Button>
        </div>
      </Modal>
      <Modal open={removing} onOpenChange={setRemoving} title={t('dag.delete')}>
        <div className="modal-body">
          {blockedDelete ? (
            <>
              <p>{t('dag.deleteBlocked')}</p>
              {used.map((n) => (
                <p key={n.id}>{n.name || t('dag.unnamed')}</p>
              ))}
              {resultUsed && <p>{t('dag.results')}</p>}
            </>
          ) : (
            <p>{node?.name || t('dag.unnamed')}</p>
          )}
        </div>
        <div className="modal-footer">
          <Button onClick={() => setRemoving(false)}>{t('取消')}</Button>
          <Button
            variant="solid"
            disabled={blockedDelete || busy}
            onClick={() => {
              const nodes = dag.nodes.filter((n) => n.id !== selected)
              changeDag({ ...dag, nodes })
              setSelected(nodes[0]?.id ?? '')
              setRemoving(false)
              setMobile('list')
            }}
          >
            {t('dag.delete')}
          </Button>
        </div>
      </Modal>
    </div>
  )
}
