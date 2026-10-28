import { promptText } from '../dag/prompt'
import { useId, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, time, type Agent, type DagNode, type Run, type Runner, type Task } from '../api'
import { t } from '../i18n'
import { CalendarCheck, Monitor } from '../icons'
import { IconButton } from '../ui'
import { ActionIcon } from '../actions/ActionIcon'
import { AgentLogo } from '../AgentLogo'
import { ChoicePicker } from '../ChoicePicker'
import { CopyValue } from '../runs/CopyValue'
import { CodeContent } from '../runs/CodeContent'
import { RunTable } from '../runs/RunTable'
import { DagGraph } from '../dag/DagGraph'
import { nodeName, nodeOptions, runnerName } from '../dag/identity'
import { outputs } from '../dag/model'
import './overview.css'

export function TaskOverview({
  task,
  agents = [],
  runs,
  pending,
  error,
  onHistory,
  toast,
}: {
  task: Task
  agents?: Agent[]
  runs: Run[]
  pending: boolean
  error?: Error | null
  onHistory: () => void
  toast: (message: string) => void
}) {
  useTranslation()
  const { config } = task
  const card = useRef<HTMLElement>(null)
  const [size, setSize] = useState('wide')
  useLayoutEffect(() => {
    const observer = new ResizeObserver(([entry]) => {
      setSize(
        entry.contentRect.width < 480 ? 'small' : entry.contentRect.width < 780 ? 'narrow' : 'wide',
      )
    })
    observer.observe(card.current!)
    return () => observer.disconnect()
  }, [])
  const runners = useQuery<Runner[]>({ queryKey: ['runners'], queryFn: () => api('/runners') })
  const nodes = config.dag?.nodes ?? []
  const [selected, setSelected] = useState(nodes[0]?.id)
  const node = nodes.find((item) => item.id === selected) ?? nodes[0]
  const source = config.dag ? node : config
  const [expanded, setExpanded] = useState<Record<string, boolean>>({})
  const identity = node?.id ?? task.id
  const schedule = config.schedule
  const rule =
    schedule?.cron ??
    (schedule?.every
      ? t('taskOverview.every', { duration: schedule.every })
      : schedule?.at
        ? time(schedule.at, schedule.timezone)
        : t('手动执行'))
  return (
    <section
      ref={card}
      data-size={size}
      className="task-overview content-card"
      aria-label={t('概览')}
    >
      <header className="task-overview-schedule card-header">
        <h3>{t('调度')}</h3>
        <div className="task-overview-rule">
          {schedule && <CalendarCheck size={14} />}
          <span className={schedule ? 'mono' : ''}>{rule}</span>
          {schedule && <span className="quiet text-muted">{schedule.timezone}</span>}
        </div>
        {!config.enabled ? (
          <span className="task-overview-next quiet text-muted">{t('actions.taskDisabled')}</span>
        ) : schedule ? (
          <div className="task-overview-next">
            <span className="quiet text-muted">{t('下一次执行')}</span>
            <time>{time(task.next_at, schedule.timezone)}</time>
          </div>
        ) : null}
      </header>
      <div className="task-overview-grid">
        <div className="task-overview-primary">
          {config.dag && (
            <div className="task-overview-structure">
              <DagGraph
                compact
                runners={runners.data}
                agents={agents}
                nodes={nodes}
                outputIds={outputs(config.dag)}
                selected={node?.id}
                onSelect={setSelected}
              />
            </div>
          )}
          {source ? (
            <TaskPrompt
              key={identity}
              text={promptText(source.prompt)}
              title={
                config.dag && node ? (
                  <>
                    <span className="quiet text-muted">{t('指令')}</span>
                    {nodes.length > 1 ? (
                      <ChoicePicker
                        label={t('dag.selectNode')}
                        value={node.id}
                        options={nodeOptions(nodes).map((item) => ({
                          value: item.id,
                          label: item.name,
                          description: item.description,
                          keywords: item.keywords,
                        }))}
                        searchable
                        onChange={setSelected}
                      />
                    ) : (
                      <strong>{nodeName(node)}</strong>
                    )}
                  </>
                ) : (
                  <strong>{t('指令')}</strong>
                )
              }
              expanded={!!expanded[identity]}
              onExpanded={(value) => setExpanded((old) => ({ ...old, [identity]: value }))}
              toast={toast}
            />
          ) : (
            <p className="quiet text-muted card-state">{t('taskOverview.noNodes')}</p>
          )}
        </div>
        <aside
          className="task-overview-context"
          aria-label={t(config.dag ? 'taskOverview.nodeConfig' : 'taskOverview.executionConfig')}
        >
          <h3 className="task-overview-heading">
            {t(config.dag ? 'taskOverview.nodeConfig' : 'taskOverview.executionConfig')}
          </h3>
          {source && (
            <ExecutionConfig
              source={source}
              agents={agents}
              runners={runners.data ?? []}
              timeout={config.dag ? node.timeout : config.execution.timeout}
              node={config.dag ? node : undefined}
              toast={toast}
            />
          )}
        </aside>
      </div>
      <RunTable
        title={t('最近执行')}
        runs={runs}
        agents={agents}
        embedded
        pending={pending}
        error={error}
        actions={
          <IconButton label={t('taskOverview.allHistory')} onClick={onHistory}>
            <ActionIcon name="open" />
          </IconButton>
        }
      />
    </section>
  )
}

function TaskPrompt({
  text,
  title,
  expanded,
  onExpanded,
  toast,
}: {
  text: string
  title: ReactNode
  expanded: boolean
  onExpanded: (value: boolean) => void
  toast: (message: string) => void
}) {
  const id = useId()
  const root = useRef<HTMLDivElement>(null)
  const body = useRef<HTMLPreElement>(null)
  const [overflow, setOverflow] = useState(false)
  useLayoutEffect(() => {
    const element = body.current!
    const measure = () => setOverflow(element.scrollHeight > 241)
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    return () => observer.disconnect()
  }, [text])
  function toggle() {
    onExpanded(!expanded)
    if (expanded)
      requestAnimationFrame(() => {
        const host = root.current?.closest('.detail-scroll')
        if (
          host &&
          root.current &&
          root.current.getBoundingClientRect().top < host.getBoundingClientRect().top
        )
          root.current.scrollIntoView({ block: 'start' })
      })
  }
  return (
    <div ref={root} className="task-overview-prompt">
      <div className="task-overview-prompt-head">
        <div className="task-overview-prompt-title">{title}</div>
        <CopyValue value={text} label={t('taskOverview.copyPrompt')} toast={toast} />
      </div>
      <pre
        ref={body}
        id={id}
        className={'task-prompt' + (!expanded ? ' is-preview' : '') + (overflow ? ' is-long' : '')}
      >
        {text}
      </pre>
      {overflow && (
        <button
          type="button"
          className="task-overview-expand"
          aria-expanded={expanded}
          aria-controls={id}
          onClick={toggle}
        >
          {t(expanded ? 'reading.less' : 'reading.full')}
        </button>
      )}
    </div>
  )
}

function ExecutionConfig({
  source,
  node,
  agents,
  runners,
  timeout,
  toast,
}: {
  source: Pick<DagNode, 'runner' | 'agent' | 'cwd' | 'parameters' | 'session'>
  node?: DagNode
  agents: Agent[]
  runners: Runner[]
  timeout: string | null
  toast: (message: string) => void
}) {
  const agent = agents.find(
    (item) =>
      item.id === source.agent &&
      (item.runner === source.runner || item.runner_id === source.runner),
  )
  const parameters = Object.fromEntries(
    Object.entries(source.parameters ?? {}).filter(([key]) => key !== 'model'),
  )
  const hasParameters = Object.keys(parameters).length > 0
  return (
    <dl className="task-overview-properties">
      <dt>{t('设备')}</dt>
      <dd>
        <Monitor size={14} />
        <span title={source.runner}>{runnerName(source, runners, agents)}</span>
      </dd>
      <dt>{t('Agent')}</dt>
      <dd>
        <AgentLogo name={agent?.name ?? source.agent} logo={agent?.logo} />
        <span>{agent?.name ?? source.agent}</span>
      </dd>
      <dt>{t('taskOverview.directory')}</dt>
      <dd className="mono">{source.cwd || '—'}</dd>
      <dt>{t('模型')}</dt>
      <dd>
        {source.parameters?.model || (
          <span className="quiet text-muted">{t('taskOverview.default')}</span>
        )}
      </dd>
      <dt>{t('会话')}</dt>
      <dd className={source.session?.id ? 'mono' : ''}>{source.session?.id ?? t('每次新建')}</dd>
      <dt>{t(node ? 'taskOverview.nodeTimeout' : '执行超时')}</dt>
      <dd>{timeout ?? '—'}</dd>
      {node && (
        <>
          <dt>{t('taskOverview.autoRetry')}</dt>
          <dd>
            {node.max_retries
              ? t('taskOverview.retryCount', { count: node.max_retries })
              : t('taskOverview.noRetries')}
          </dd>
        </>
      )}
      <dt>{t('taskOverview.otherParameters')}</dt>
      <dd className="task-overview-parameters-cell">
        {hasParameters ? (
          <details className="task-overview-parameters">
            <summary>
              {t('taskOverview.parameterCount', { count: Object.keys(parameters).length })}
            </summary>
            <div className="task-overview-parameter-copy">
              <CopyValue value={JSON.stringify(parameters, null, 2)} toast={toast} />
            </div>
            <CodeContent text={JSON.stringify(parameters, null, 2)} json />
          </details>
        ) : (
          <span className="quiet text-muted">—</span>
        )}
      </dd>
    </dl>
  )
}
