import { validDuration } from './duration'
import { ErrorToast } from './Toast'
import { t as tr, parameterLabel } from './i18n'
import { useTranslation } from 'react-i18next'
import { useState, useRef, useEffect, type ReactNode } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Clock, ChevronRight } from './icons'
import { api, time, type Task, type TaskConfig, type Runner } from './api'
import { Button, Field, Modal, focusEditorField } from './ui'
import { ChoicePicker } from './ChoicePicker'
import { AgentLogo } from './AgentLogo'
import { DirectoryPicker } from './DirectoryPicker'
import { ModelPicker, ParameterFields } from './ModelPicker'
import { createTaskId, taskIdSuffix } from './taskId'
import { ScheduleSummary } from './ScheduleSummary'
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
  const timeConfig = config
  const baseline = useRef(JSON.stringify(config))
  const [error, setError] = useState('')
  const [focusField, setFocusField] = useState<string | null>(null)
  const [scheduleType, setScheduleType] = useState(
    config.schedule?.every
      ? 'every'
      : config.schedule?.at
        ? 'at'
        : config.schedule
          ? 'cron'
          : 'manual',
  )
  const selectedRunner = runners.data?.find(
    (r) => r.name === config.runner || r.id === config.runner,
  )
  const selectedAgent = selectedRunner?.agents.find((a) => a.id === config.agent)
  const hasAdvanced =
    !!selectedAgent?.session_supported ||
    !!config.session ||
    Object.keys(selectedAgent?.parameters ?? {}).some((key) => key !== 'model')
  const preview = useQuery<string[]>({
    queryKey: ['preview', config.schedule],
    queryFn: () => api('/schedule/preview', 'POST', config.schedule),
    enabled: step === 'time' && !!config.schedule,
    retry: false,
  })
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
        if (!validDuration(timeConfig.execution.timeout ?? '1h'))
          throw new Error(tr('请填写有效的执行超时（1 秒至 366 天）'))
        if (!validDuration(timeConfig.execution.start_grace))
          throw new Error(tr('请填写有效的允许延迟（1 秒至 366 天）'))
        if (timeConfig.schedule) await api('/schedule/preview', 'POST', timeConfig.schedule)
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
        <div className="modal-body">
          <fieldset className="editor-fields" disabled={save.isPending}>
            <div
              className={embedded ? 'creation-task-content' : undefined}
              hidden={!embedded && step !== 'task'}
              inert={!embedded && step !== 'task'}
            >
              <div className="task-core">
                <Field label={tr('任务名称')}>
                  <input
                    autoFocus
                    value={config.name}
                    onChange={(e) => change('name', e.target.value)}
                    required
                    placeholder={tr('每日仓库检查')}
                  />
                </Field>
                {
                  <>
                    <Field label={tr('指令')}>
                      <textarea
                        value={config.prompt}
                        onChange={(e) => change('prompt', e.target.value)}
                        required
                        rows={4}
                        placeholder={tr('描述希望 Agent 完成的工作…')}
                      />
                    </Field>
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
                              value === c.agent
                                ? c
                                : { ...c, agent: value, parameters: {}, session: null },
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
                }
              </div>
              {hasAdvanced && (
                <details className="advanced-settings">
                  <summary>
                    <ChevronRight size={14} />
                    <span>{tr('高级设置')}</span>
                  </summary>
                  <div className="advanced-body">
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
            </div>
            <details
              className={embedded ? 'advanced-settings creation-schedule' : undefined}
              open={embedded ? step === 'time' : true}
              hidden={!embedded && step !== 'time'}
              inert={!embedded && step !== 'time'}
              onToggle={
                embedded
                  ? (event) => setStep(event.currentTarget.open ? 'time' : 'task')
                  : undefined
              }
            >
              {embedded && (
                <summary>
                  <span>{tr('creation.schedule')}</span>
                  <ScheduleSummary schedule={config.schedule} enabled={config.enabled} />
                </summary>
              )}
              <div className="form-grid">
                <Field label={tr('类型')}>
                  <ChoicePicker
                    label={tr('类型')}
                    value={scheduleType}
                    disabled={save.isPending}
                    options={[
                      { value: 'cron', label: 'Cron' },
                      { value: 'every', label: tr('固定间隔') },
                      { value: 'at', label: tr('一次性执行') },
                      { value: 'manual', label: tr('手动执行') },
                    ]}
                    onChange={(value) => {
                      const type = value
                      setScheduleType(type)
                      change(
                        'schedule',
                        type === 'manual'
                          ? null
                          : {
                              timezone: config.schedule?.timezone ?? defaultTimezone,
                              ...(type === 'cron'
                                ? { cron: '0 9 * * *' }
                                : type === 'every'
                                  ? { every: '1h' }
                                  : { at: new Date(Date.now() + 3600000).toISOString() }),
                            },
                      )
                    }}
                  />
                </Field>
                {config.schedule && (
                  <Field label={tr('时区')}>
                    <input
                      value={config.schedule.timezone}
                      onChange={(e) =>
                        change('schedule', { ...config.schedule, timezone: e.target.value })
                      }
                    />
                  </Field>
                )}
              </div>
              {config.schedule && (
                <Field
                  label={
                    scheduleType === 'cron'
                      ? tr('Cron 表达式')
                      : scheduleType === 'every'
                        ? tr('间隔')
                        : tr('执行时间')
                  }
                  hint={scheduleType === 'cron' ? tr('分钟 小时 日期 月份 星期') : undefined}
                >
                  <input
                    className="mono"
                    value={
                      config.schedule.cron ?? config.schedule.every ?? config.schedule.at ?? ''
                    }
                    onChange={(e) =>
                      change('schedule', { ...config.schedule, [scheduleType]: e.target.value })
                    }
                    required
                  />
                </Field>
              )}
              {config.schedule && (
                <div className="preview">
                  <Clock size={13} />
                  <div>
                    <span>{tr('接下来的执行时间')}</span>
                    {preview.error ? (
                      <ErrorToast>{preview.error.message}</ErrorToast>
                    ) : (
                      preview.data?.map((t) => (
                        <small key={t}>
                          {time(t, config.schedule!.timezone || defaultTimezone)}{' '}
                          {config.schedule!.timezone || defaultTimezone}
                        </small>
                      ))
                    )}
                  </div>
                </div>
              )}
              {
                <details className="advanced-settings">
                  <summary>
                    <ChevronRight size={14} />
                    <span>{tr('高级设置')}</span>
                  </summary>
                  <div className="advanced-body">
                    <div className="form-grid">
                      <Field label={tr('执行超时')}>
                        <input
                          value={config.execution.timeout ?? '1h'}
                          onChange={(e) =>
                            change('execution', { ...config.execution, timeout: e.target.value })
                          }
                        />
                      </Field>
                      <Field label={tr('允许延迟')}>
                        <input
                          value={config.execution.start_grace}
                          onChange={(e) =>
                            change('execution', {
                              ...config.execution,
                              start_grace: e.target.value,
                            })
                          }
                        />
                      </Field>
                      <Field label={tr('错过执行')}>
                        <ChoicePicker
                          label={tr('错过执行')}
                          value={config.execution.misfire}
                          disabled={save.isPending}
                          options={[
                            { value: 'latest', label: tr('补最近一次') },
                            { value: 'skip', label: tr('跳过') },
                          ]}
                          onChange={(value) =>
                            change('execution', { ...config.execution, misfire: value })
                          }
                        />
                      </Field>
                      <Field label={tr('执行重叠')}>
                        <ChoicePicker
                          label={tr('执行重叠')}
                          value={config.execution.overlap}
                          disabled={save.isPending}
                          options={[
                            { value: 'skip', label: tr('跳过新触发') },
                            { value: 'allow', label: tr('允许并行') },
                          ]}
                          onChange={(value) =>
                            change('execution', { ...config.execution, overlap: value })
                          }
                        />
                      </Field>
                    </div>
                  </div>
                </details>
              }
            </details>
          </fieldset>
        </div>
        <div className="modal-footer">
          {error && <ErrorToast>{error}</ErrorToast>}
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
