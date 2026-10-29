import { ErrorToast } from './Toast'
import { t as tr, parameterLabel } from './i18n'
import { useTranslation } from 'react-i18next'
import { useState, useRef, useEffect, type CSSProperties, type ReactNode } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { ChevronRight } from './icons'
import { api, type Task, type TaskConfig, type Runner, type DagIssue } from './api'
import { DetailTabs } from './DetailTabs'
import { TimeFields } from './dag/TimeFields'
import { Button, Field, Modal, focusEditorField } from './ui'
import { ChoicePicker } from './ChoicePicker'
import { AgentLogo } from './AgentLogo'
import { DirectoryPicker } from './DirectoryPicker'
import { ModelPicker, ParameterFields } from './ModelPicker'
import { createTaskId, taskIdSuffix } from './taskId'
import { PaneResizeHandle } from './PaneResizeHandle'
import { SessionBinding } from './sessions/SessionBinding'
export type TaskDefaults = {
  runner: string
  agent?: string
  cwd?: string
  parameters?: Record<string, string>
  session?: { id: string } | null
}
export function TaskEditor({
  task,
  defaultTimezone,
  defaults,
  onClose,
  embedded = false,
  onStateChange,
  onCreated,
}: {
  task?: Task
  defaultTimezone: string
  defaults?: TaskDefaults
  onClose: () => void
  onCreated?: (id: string) => void
  embedded?: boolean
  onStateChange?: (dirty: boolean, busy: boolean) => void
}) {
  useTranslation()
  const form = useRef<HTMLFormElement>(null)
  const timeValidation = useRef<() => DagIssue[]>(() => [])
  const workspace = useRef<HTMLDivElement>(null)
  const [workspaceWidth, setWorkspaceWidth] = useState(0)
  const [sideSize, setSideSize] = useState(380)
  useEffect(() => {
    if (!embedded || !workspace.current) return
    const element = workspace.current
    const measure = () => setWorkspaceWidth(element.getBoundingClientRect().width)
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    return () => observer.disconnect()
  }, [embedded])
  const client = useQueryClient()
  const runners = useQuery<Runner[]>({
    queryKey: ['runners'],
    queryFn: () => api('/runners'),
    refetchInterval: 3000,
  })
  const [step, setStep] = useState<'task' | 'time'>('task')
  const [suffix] = useState(taskIdSuffix)
  const [modelOptions, setModelOptions] = useState<Record<string, string[]>>({})
  const [config, setConfig] = useState<TaskConfig>(() =>
    task
      ? { ...task.config, parameters: task.config.parameters ?? {} }
      : {
          version: 1,
          id: '',
          name: '',
          runner: defaults?.runner ?? '',
          agent: defaults?.agent ?? '',
          cwd: defaults?.cwd ?? '',
          prompt: '',
          parameters: defaults?.parameters ?? {},
          session: defaults?.session ?? null,
          enabled: true,
          schedule: { cron: '0 9 * * *', timezone: defaultTimezone },
          execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
        },
  )
  const baseline = useRef(JSON.stringify(config))
  const [error, setError] = useState('')
  const [focusField, setFocusField] = useState<string | null>(null)
  const selectedRunner = runners.data?.find(
    (r) => r.name === config.runner || r.id === config.runner,
  )
  const selectedAgent = selectedRunner?.agents.find((a) => a.id === config.agent)
  const hasAdvanced =
    !!selectedAgent?.session_supported ||
    !!config.session ||
    Object.keys(selectedAgent?.parameters ?? {}).some((key) => key !== 'model')
  const change = (key: keyof TaskConfig, value: any) =>
    setConfig((c) => ({
      ...c,
      [key]: value,
      ...(key === 'cwd' && value !== c.cwd ? { session: null } : {}),
      ...(key === 'name' && !task ? { id: createTaskId(value, suffix) } : {}),
    }))
  function invalid(message: string, field: string): never {
    throw Object.assign(new Error(message), { field })
  }
  const validateTask = () => {
    if (!config.name.trim()) invalid(tr('请填写任务名称'), tr('任务名称'))
    if (!config.prompt.trim()) invalid(tr('请填写指令'), tr('指令'))
    if (!selectedRunner || selectedRunner.revoked) invalid(tr('请选择 Runner'), tr('Runner'))
    const agent = selectedRunner.agents.find((a) => a.id === config.agent)
    if (!agent) invalid(tr('请选择该 Runner 的 Agent'), tr('Agent'))
    if (!config.cwd.trim()) invalid(tr('请选择工作目录'), tr('工作目录'))
    if (config.session && !config.session.id.trim()) invalid(tr('请填写会话 ID'), tr('会话'))
    if (config.session && !agent.session_supported)
      invalid(tr('该 Agent 不支持绑定会话'), tr('会话'))
    for (const [key, value] of Object.entries(config.parameters)) {
      const spec = agent.parameters[key]
      if (!spec) throw new Error(tr('未知参数：{{v0}}', { v0: key }))
      if (spec.type !== 'string' || new TextEncoder().encode(value).length > 4096)
        throw new Error(tr('参数无效：{{v0}}', { v0: parameterLabel(spec.label || key) }))
      const resolved = value || spec.default
      if (resolved && spec.enum?.length && !spec.enum.includes(resolved))
        throw new Error(tr('请选择有效的{{v0}}', { v0: parameterLabel(spec.label || key) }))
    }
  }
  const advance = () => {
    if (!save.isPending) setStep('time')
  }
  const save = useMutation({
    mutationFn: async () => {
      try {
        validateTask()
      } catch (e) {
        setStep('task')
        setFocusField((e as Error & { field?: string }).field ?? '')
        throw e
      }
      try {
        const issue = timeValidation.current()[0]
        if (issue) throw new Error(issue.message)
        if (config.schedule) await api('/schedule/preview', 'POST', config.schedule)
      } catch (e) {
        setStep('time')
        throw e
      }
      const next = { ...config, id: task?.id ?? createTaskId(config.name, suffix) }
      return api<Task>(`/tasks/${encodeURIComponent(next.id)}`, 'PUT', {
        config: next,
        expected_revision: task?.revision ?? null,
      })
    },
    onSuccess: (result) => {
      baseline.current = JSON.stringify(config)
      client.invalidateQueries({ queryKey: ['tasks'] })
      if (onCreated) onCreated(result.id)
      else onClose()
    },
    onError: (e) => setError(e.message),
  })
  useEffect(() => {
    if (focusField === null || save.isPending) return
    focusEditorField(form.current, focusField)
    setFocusField(null)
  }, [focusField, save.isPending, step])
  useEffect(() => {
    onStateChange?.(JSON.stringify(config) !== baseline.current, save.isPending)
  }, [config, save.isPending, onStateChange])
  const split = workspaceWidth >= 800
  const maxSideWidth = Math.max(300, Math.min(640, workspaceWidth - 320))
  const sideWidth = Math.min(sideSize, maxSideWidth)
  const nameField = (
    <Field label={tr('任务名称')}>
      <input
        autoFocus
        value={config.name}
        onChange={(e) => change('name', e.target.value)}
        required
        placeholder={tr('每日仓库检查')}
      />
    </Field>
  )
  const promptField = (
    <Field label={tr('指令')}>
      <textarea
        value={config.prompt}
        onChange={(e) => change('prompt', e.target.value)}
        required
        rows={4}
        placeholder={tr('描述希望 Agent 完成的工作…')}
      />
    </Field>
  )
  const environmentFields = (
    <>
      <div className="form-grid">
        <Field label={tr('Runner')}>
          <ChoicePicker
            label={tr('Runner')}
            searchable
            placeholder={tr('选择设备')}
            disabled={save.isPending}
            value={selectedRunner?.name ?? config.runner}
            options={(runners.data ?? [])
              .filter((r) => !r.revoked)
              .map((r) => ({
                value: r.name,
                label: r.name,
                status: r.online ? undefined : tr('离线'),
              }))}
            onChange={(value) =>
              value === selectedRunner?.name
                ? undefined
                : setConfig((c) => ({
                    ...c,
                    runner: value,
                    cwd: '',
                    agent: '',
                    parameters: {},
                    session: null,
                  }))
            }
          />
        </Field>
        <Field label={tr('Agent')}>
          <ChoicePicker
            label={tr('Agent')}
            searchable
            placeholder={tr('选择 Agent')}
            disabled={save.isPending || !selectedRunner}
            value={config.agent}
            selectionLabel={selectedAgent?.name}
            options={(selectedRunner?.agents ?? [])
              .filter((a) => a.available)
              .map((a) => ({
                value: a.id,
                label: a.name,
                keywords: a.id,
                icon: <AgentLogo name={a.name} logo={a.logo} />,
              }))}
            onChange={(value) =>
              setConfig((c) =>
                value === c.agent ? c : { ...c, agent: value, parameters: {}, session: null },
              )
            }
          />
        </Field>
      </div>
      <DirectoryPicker
        runner={selectedRunner}
        value={config.cwd}
        onChange={(value) => change('cwd', value)}
      />
      <ModelPicker
        key={`${selectedRunner?.id}/${config.agent}`}
        runner={selectedRunner}
        agent={selectedRunner?.agents.find((a) => a.id === config.agent)}
        cwd={config.cwd}
        values={config.parameters}
        onChange={(values) => change('parameters', values)}
        onOptionsChange={setModelOptions}
        submitting={save.isPending}
      />
    </>
  )
  const advancedFields = (
    <>
      {hasAdvanced && (
        <details
          className={
            embedded
              ? 'advanced-settings creation-section creation-task-advanced'
              : 'advanced-settings'
          }
        >
          <summary>
            <ChevronRight size={14} />
            <span>{tr('高级设置')}</span>
          </summary>
          <div className={embedded ? 'creation-section-body' : 'advanced-body'}>
            {(selectedAgent?.session_supported || config.session) && (
              <SessionBinding
                runner={selectedRunner}
                agent={selectedAgent}
                cwd={config.cwd}
                value={config.session ?? null}
                disabled={save.isPending}
                onChange={(session) => change('session', session)}
              />
            )}
            <ParameterFields
              disabled={save.isPending}
              agent={selectedRunner?.agents.find((a) => a.id === config.agent)}
              values={config.parameters}
              options={modelOptions}
              onChange={(values) => change('parameters', values)}
            />
          </div>
        </details>
      )}
    </>
  )
  const scheduleFields = (
    <TimeFields
      config={config}
      timezone={defaultTimezone}
      disabled={save.isPending}
      active={step === 'time'}
      onChange={setConfig}
      validation={timeValidation}
    />
  )
  return (
    <TaskEditorSurface
      embedded={embedded}
      open
      onOpenChange={(v) => !v && !save.isPending && onClose()}
      title={task ? tr('编辑任务') : tr('新建任务')}
      wide
    >
      {!embedded && (
        <div className="tabs modal-tabs" role="tablist" aria-label={tr('任务编辑')}>
          <button
            type="button"
            role="tab"
            aria-selected={step === 'task'}
            className={step === 'task' ? 'active' : ''}
            disabled={save.isPending}
            onClick={() => {
              setStep('task')
              setError('')
            }}
          >
            {tr('任务')}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={step === 'time'}
            className={step === 'time' ? 'active' : ''}
            disabled={save.isPending}
            onClick={advance}
          >
            {tr('时间')}
          </button>
        </div>
      )}
      <form
        ref={form}
        noValidate
        onSubmit={(e) => {
          e.preventDefault()
          setError('')
          if (save.isPending) return
          if (!embedded && step === 'task') advance()
          else save.mutate()
        }}
      >
        {embedded ? (
          <fieldset className="editor-fields creation-task-fields" disabled={save.isPending}>
            <div className="creation-name-row">{nameField}</div>
            <DetailTabs
              idPrefix="creation-task-section"
              className="creation-section-tabs"
              label={tr('任务编辑')}
              value={step}
              disabled={save.isPending}
              tabs={[
                { key: 'task', label: tr('creation.content') },
                { key: 'time', label: tr('creation.schedule') },
              ]}
              onChange={setStep}
            />
            <div
              className="creation-workspace"
              id="creation-task-section-task-panel"
              role="tabpanel"
              aria-labelledby="creation-task-section-task-tab"
              hidden={step !== 'task'}
              inert={step !== 'task'}
              ref={workspace}
              style={{ '--creation-side-width': `${sideWidth}px` } as CSSProperties}
            >
              <section className="creation-main-pane creation-task-instruction">
                {promptField}
              </section>
              <aside className="creation-side-pane">
                <div className="creation-task-properties">{environmentFields}</div>
                {advancedFields}
              </aside>
              {split && (
                <PaneResizeHandle
                  className="creation-pane-resizer"
                  label={tr('creation.resizeConfiguration')}
                  edge="right"
                  width={sideWidth}
                  min={300}
                  max={maxSideWidth}
                  onResize={(width) => !save.isPending && setSideSize(width)}
                  onReset={() => !save.isPending && setSideSize(380)}
                  style={{ right: sideWidth - 4, top: 0, bottom: 0 }}
                />
              )}
            </div>
            <div
              className="creation-schedule-scroll"
              id="creation-task-section-time-panel"
              role="tabpanel"
              aria-labelledby="creation-task-section-time-tab"
              hidden={step !== 'time'}
              inert={step !== 'time'}
            >
              {scheduleFields}
            </div>
          </fieldset>
        ) : (
          <div className="modal-body">
            <fieldset className="editor-fields" disabled={save.isPending}>
              <div hidden={step !== 'task'} inert={step !== 'task'}>
                <div className="task-core">
                  {nameField}
                  {promptField}
                  {environmentFields}
                </div>
                {advancedFields}
              </div>
              <div hidden={step !== 'time'} inert={step !== 'time'}>
                {scheduleFields}
              </div>
            </fieldset>
          </div>
        )}
        <div className={embedded ? 'modal-footer creation-footer' : 'modal-footer'}>
          {error && <ErrorToast error={error} />}
          <Button type="button" disabled={save.isPending} onClick={onClose}>
            {tr('取消')}
          </Button>
          {!embedded && step === 'time' && (
            <Button
              type="button"
              disabled={save.isPending}
              onClick={() => {
                setStep('task')
                setError('')
              }}
            >
              {tr('上一步')}
            </Button>
          )}
          <Button variant="solid" type="submit" disabled={save.isPending}>
            {save.isPending
              ? tr('正在保存…')
              : embedded
                ? tr('creation.create')
                : step === 'task'
                  ? tr('下一步')
                  : tr('保存任务')}
          </Button>
        </div>
      </form>
    </TaskEditorSurface>
  )
}

function TaskEditorSurface({
  embedded,
  children,
  ...props
}: React.ComponentProps<typeof Modal> & { embedded: boolean; children: ReactNode }) {
  return embedded ? (
    <div className="creation-task-form">{children}</div>
  ) : (
    <Modal {...props}>{children}</Modal>
  )
}
