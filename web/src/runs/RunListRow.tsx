import { useTranslation } from 'react-i18next'
import { Monitor, ChevronRight, Clock, GitBranch } from 'lucide-react'
import { t } from '../i18n'
import { type Agent, type Run, time } from '../api'
import { Status } from '../Status'
import { AgentLogo } from '../AgentLogo'
export function runAgent(run: Pick<Run, 'config'>, agents?: Agent[]) {
  return agents?.find(
    (a) =>
      a.id === run.config.agent &&
      (a.runner === run.config.runner || a.runner_id === run.config.runner),
  )
}
export function RunIdentity({ run, agents }: { run: Pick<Run, 'config'>; agents?: Agent[] }) {
  useTranslation()
  if (run.config.dag)
    return (
      <span className="run-identity">
        <span className="run-device">
          <GitBranch size={13} />
          DAG
        </span>
        <span className="quiet">{t('dag.nodeCount', { count: run.config.dag.nodes.length })}</span>
      </span>
    )
  return (
    <div className="run-identity">
      <RunDevice run={run} agents={agents} />
      <RunAgent run={run} agents={agents} />
    </div>
  )
}
export function RunDevice({ run, agents }: { run: Pick<Run, 'config'>; agents?: Agent[] }) {
  useTranslation()
  const agent = runAgent(run, agents)
  return (
    <span className="run-device" title={agent?.runner ?? run.config.runner}>
      <Monitor size={13} />
      <span>{agent?.runner ?? run.config.runner}</span>
    </span>
  )
}
export function RunAgent({ run, agents }: { run: Pick<Run, 'config'>; agents?: Agent[] }) {
  useTranslation()
  const agent = runAgent(run, agents)
  return (
    <span className="run-agent" title={agent?.name ?? run.config.agent}>
      <AgentLogo name={agent?.name ?? run.config.agent} logo={agent?.logo} />
      <span>{agent?.name ?? run.config.agent}</span>
    </span>
  )
}
export function RunListRow({
  run,
  agents,
  selected,
  onSelect,
}: {
  run: Run
  agents?: Agent[]
  selected: boolean
  onSelect: () => void
}) {
  useTranslation()
  return (
    <button
      className={`task-row run-row ${selected ? 'active' : ''}`}
      aria-current={selected ? 'page' : undefined}
      onClick={onSelect}
    >
      <div className="row-title">
        <span title={run.config.name}>{run.config.name}</span>
        <Status state={run.state} />
      </div>
      <div className="row-meta">
        <RunIdentity run={run} agents={agents} />
      </div>
      <div className="row-meta next">
        <Clock size={12} />
        {time(run.started_at ?? run.created_at)}
        <ChevronRight size={13} />
      </div>
    </button>
  )
}
