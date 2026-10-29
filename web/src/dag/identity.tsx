import { type DagNode, type Runner, type Agent } from '../api/types'
import { t } from '../i18n'
export const nodeName = (node: Pick<DagNode, 'name'>) => node.name || t('dag.unnamed')
export const shortId = (id: string) => (id.length > 16 ? id.slice(0, 16) : id)
export function runnerName(node: Pick<DagNode, 'runner'>, runners: Runner[], agents: Agent[]) {
  return (
    runners.find((r) => r.id === node.runner || r.name === node.runner)?.name ??
    agents.find((a) => a.runner_id === node.runner || a.runner === node.runner)?.runner ??
    (/^[0-9a-f-]{36}$/i.test(node.runner) ? t('dag.deviceUnavailable') : node.runner || '—')
  )
}
export const nodeOptions = (nodes: DagNode[]) =>
  nodes.map((n) => ({
    id: n.id,
    name: nodeName(n),
    description: shortId(n.id),
    keywords: n.id,
  }))
