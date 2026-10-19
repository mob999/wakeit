import { type Dag, type DagNode, type DagIssue } from '../api'
import { t } from '../i18n'

function nextNodeName(nodes: DagNode[]) {
  let index = 1
  while (nodes.some((n) => n.name === t('dag.defaultNode', { number: index }))) index++
  return t('dag.defaultNode', { number: index })
}
export const newNode = (nodes: DagNode[] = []): DagNode => ({
  id: `node_${crypto.randomUUID().replaceAll('-', '').slice(0, 10)}`,
  name: nextNodeName(nodes),
  pre: [],
  runner: '',
  agent: '',
  cwd: '',
  prompt: '',
  parameters: {},
  session: null,
  timeout: '1h',
})
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
    for (const key of ['name', 'runner', 'agent', 'cwd', 'prompt'] as const)
      if (!node[key]?.trim())
        result.push({
          node_id: node.id,
          path: `dag.nodes.${node.id}.${key}`,
          message: t('dag.required', { field: t(`dag.field.${key}`) }),
        })
    if (ancestors(dag, node.id).has(node.id))
      result.push({ node_id: node.id, path: `dag.nodes.${node.id}.pre`, message: t('dag.cycle') })
    const upstream = ancestors(dag, node.id)
    for (const match of node.prompt.matchAll(/\{\{\s*nodes\.([a-zA-Z0-9_-]+)\.result\s*\}\}/g))
      if (!upstream.has(match[1]))
        result.push({
          node_id: node.id,
          path: `dag.nodes.${node.id}.prompt`,
          message: t('dag.notAncestor', { name: match[1] }),
        })
  }
  if (!dag.nodes.length) result.push({ path: 'dag.nodes', message: t('dag.needNode') })
  if (!outputs(dag).length) result.push({ path: 'dag.results', message: t('dag.needResult') })
  return result
}
export function referencesTo(dag: Dag, id: string) {
  return dag.nodes.filter(
    (n) =>
      n.id !== id &&
      (n.pre.includes(id) ||
        new RegExp(`\\{\\{\\s*nodes\\.${id}\\.result\\s*\\}\\}`).test(n.prompt)),
  )
}

export function referenceLocations(dag: Dag, id: string) {
  const rows: { node_id?: string; name: string; field: string; kind: string }[] = []
  for (const node of dag.nodes) {
    if (node.pre.includes(id))
      rows.push({ node_id: node.id, name: node.name, field: 'pre', kind: t('dag.pre') })
    if (
      Array.from(node.prompt.matchAll(/\{\{\s*nodes\.([a-zA-Z0-9_-]+)\.result\s*\}\}/g)).some(
        (m) => m[1] === id,
      )
    )
      rows.push({ node_id: node.id, name: node.name, field: 'prompt', kind: t('dag.insertResult') })
  }
  if (dag.results?.includes(id))
    rows.push({ name: t('dag.results'), field: 'results', kind: t('dag.outputNode') })
  return rows
}
