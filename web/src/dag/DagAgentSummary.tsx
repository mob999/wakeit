import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api } from '../api/client'
import { type Agent, type DagNode } from '../api/types'
import { GitBranch } from '../components/icons'
import { locale, t } from '../i18n'
import { TooltipTarget } from '../components/ui'
import './DagAgentSummary.css'

export function DagAgentSummary({
  nodes,
  count = nodes?.length ?? 0,
  agents,
}: {
  nodes?: Pick<DagNode, 'runner' | 'agent'>[]
  count?: number
  agents?: Agent[]
}) {
  useTranslation()
  const available = useQuery({
    queryKey: ['agents'],
    queryFn: ({ signal }) => api<Agent[]>('/agents', 'GET', undefined, undefined, signal),
    enabled: agents === undefined && !!nodes?.length,
    staleTime: 30_000,
  })
  const metadata = agents ?? available.data
  const names = Array.from(
    new Set(
      nodes?.map(
        (node) =>
          metadata?.find(
            (agent) =>
              agent.id === node.agent &&
              (agent.runner_id === node.runner || agent.runner === node.runner),
          )?.name ?? node.agent,
      ),
    ),
  )
  const list = new Intl.ListFormat(locale(), { style: 'long', type: 'conjunction' })
  const describe = (values: string[]) =>
    values.length
      ? t('dag.agentNodes', { agents: list.format(values), count })
      : t('dag.nodeCount', { count })
  return (
    <TooltipTarget label={describe(names)}>
      <span className="dag-agent-summary">
        <GitBranch size={13} />
        <span>{describe(names.slice(0, 2))}</span>
      </span>
    </TooltipTarget>
  )
}
