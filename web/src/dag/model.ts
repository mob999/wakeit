import { promptReferences, promptBlank } from './prompt'
import { type Dag, type DagIssue } from '../api'
import { t } from '../i18n'

export function ancestors(dag: Dag, id: string) {
  const found = new Set<string>(),
    pending = [...(dag.nodes.find((n) => n.id === id)?.pre ?? [])]
  while (pending.length) {
    const next = pending.pop()!
    if (found.has(next)) continue
    found.add(next)
    pending.push(...(dag.nodes.find((n) => n.id === next)?.pre ?? []))
  }
  return found
}
export const outputs = (dag: Dag) =>
  dag.results ??
  dag.nodes.filter((n) => !dag.nodes.some((other) => other.pre.includes(n.id))).map((n) => n.id)
export function issues(dag: Dag): DagIssue[] {
  const result: DagIssue[] = []
  for (const node of dag.nodes) {
    for (const key of ['name', 'runner', 'agent', 'cwd'] as const)
      if (!node[key]?.trim())
        result.push({
          node_id: node.id,
          path: `dag.nodes.${node.id}.${key}`,
          message: t('dag.required', { field: t(`dag.field.${key}`) }),
        })
    if (promptBlank(node.prompt))
      result.push({
        node_id: node.id,
        path: `dag.nodes.${node.id}.prompt`,
        message: t('dag.required', { field: t('dag.field.prompt') }),
      })
    if (
      !Number.isInteger(node.max_retries ?? 0) ||
      (node.max_retries ?? 0) < 0 ||
      (node.max_retries ?? 0) > 10
    )
      result.push({
        node_id: node.id,
        path: `dag.nodes.${node.id}.max_retries`,
        message: t('retry.invalid'),
      })
    if (ancestors(dag, node.id).has(node.id))
      result.push({ node_id: node.id, path: `dag.nodes.${node.id}.pre`, message: t('dag.cycle') })
    if (
      new Set(node.pre).size !== node.pre.length ||
      node.pre.some((id) => !dag.nodes.some((n) => n.id === id))
    )
      result.push({
        node_id: node.id,
        path: `dag.nodes.${node.id}.pre`,
        message: t('dag.connection.missing'),
      })
    const upstream = ancestors(dag, node.id)
    for (const id of promptReferences(node.prompt))
      if (!upstream.has(id))
        result.push({
          node_id: node.id,
          path: `dag.nodes.${node.id}.prompt`,
          message: t('dag.notAncestor', { name: id }),
        })
  }
  if (!dag.nodes.length) result.push({ path: 'dag.nodes', message: t('dag.needNode') })
  if (!outputs(dag).length) result.push({ path: 'dag.results', message: t('dag.needResult') })
  if (dag.results?.some((id) => !dag.nodes.some((n) => n.id === id)))
    result.push({ path: 'dag.results', message: t('dag.missingOutput') })
  return result
}
