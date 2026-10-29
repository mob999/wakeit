import type { Agent, Dag, DagNode, EditorLayout, Runner } from '../api/types'
import { ancestors } from './model'
export type Point = { x: number; y: number }
export type GraphDraft = { dag: Dag; layout: EditorLayout }
export type AgentChoice = { runner: Runner; agent: Agent }
export const positionOf = (layout: EditorLayout, id: string): Point | undefined =>
  Object.hasOwn(layout.positions, id) ? layout.positions[id] : undefined
export const emptyLayout = (): EditorLayout => ({ version: 1, positions: {} })
const equal = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)
type Patch =
  | { kind: 'node'; id: string; value?: DagNode; index: number }
  | { kind: 'field'; id: string; key: keyof DagNode; value: unknown }
  | { kind: 'position'; id: string; value?: Point }
  | { kind: 'results'; value: Dag['results'] }
function diff(from: GraphDraft, to: GraphDraft): Patch[] {
  const patches: Patch[] = []
  for (const id of new Set([...from.dag.nodes, ...to.dag.nodes].map((n) => n.id))) {
    const a = from.dag.nodes.find((n) => n.id === id),
      b = to.dag.nodes.find((n) => n.id === id)
    if (!a || !b)
      patches.push({
        kind: 'node',
        id,
        value: b && structuredClone(b),
        index: to.dag.nodes.findIndex((n) => n.id === id),
      })
    else
      for (const key of Object.keys(b) as (keyof DagNode)[])
        if (!equal(a[key], b[key]))
          patches.push({ kind: 'field', id, key, value: structuredClone(b[key]) })
  }
  for (const id of new Set([
    ...Object.keys(from.layout.positions),
    ...Object.keys(to.layout.positions),
  ]))
    if (!equal(positionOf(from.layout, id), positionOf(to.layout, id)))
      patches.push({
        kind: 'position',
        id,
        value: positionOf(to.layout, id) && { ...positionOf(to.layout, id)! },
      })
  if (!equal(from.dag.results, to.dag.results))
    patches.push({ kind: 'results', value: to.dag.results && [...to.dag.results] })
  return patches
}
function apply(current: GraphDraft, patches: Patch[]): GraphDraft {
  const next = structuredClone(current)
  for (const p of patches) {
    if (p.kind === 'results') next.dag.results = p.value && [...p.value]
    if (p.kind === 'position') {
      if (p.value) next.layout.positions[p.id] = { ...p.value }
      else delete next.layout.positions[p.id]
    }
    if (p.kind === 'node') {
      next.dag.nodes = next.dag.nodes.filter((n) => n.id !== p.id)
      if (p.value) next.dag.nodes.splice(Math.max(0, p.index), 0, structuredClone(p.value))
    }
    if (p.kind === 'field') {
      const node = next.dag.nodes.find((n) => n.id === p.id)
      if (node) Object.assign(node, { [p.key]: structuredClone(p.value) })
    }
  }
  return next
}
export class GraphHistory {
  past: Patch[][] = []
  future: Patch[][] = []
  record(before: GraphDraft, after: GraphDraft) {
    const patches = diff(after, before)
    if (!patches.length) return false
    this.past.push(patches)
    if (this.past.length > 100) this.past.shift()
    this.future = []
    return true
  }
  undo(current: GraphDraft) {
    const patches = this.past.pop()
    if (!patches) return current
    const next = apply(current, patches)
    this.future.push(diff(next, current))
    return next
  }
  redo(current: GraphDraft) {
    const patches = this.future.pop()
    if (!patches) return current
    const next = apply(current, patches)
    this.past.push(diff(next, current))
    return next
  }
}
export function connectionIssue(
  dag: Dag,
  source: string,
  target: string,
  previous?: { source: string; target: string },
): 'self' | 'duplicate' | 'cycle' | 'missing' | null {
  if (!dag.nodes.some((n) => n.id === source) || !dag.nodes.some((n) => n.id === target))
    return 'missing'
  if (source === target) return 'self'
  const graph = previous
    ? {
        ...dag,
        nodes: dag.nodes.map((n) =>
          n.id === previous.target
            ? { ...n, pre: n.pre.filter((id) => id !== previous.source) }
            : n,
        ),
      }
    : dag
  if (graph.nodes.find((n) => n.id === target)!.pre.includes(source)) return 'duplicate'
  return ancestors(graph, source).has(target) ? 'cycle' : null
}
export function connect(
  dag: Dag,
  source: string,
  target: string,
  previous?: { source: string; target: string },
): Dag {
  return {
    ...dag,
    nodes: dag.nodes.map((n) => {
      let pre =
        previous && n.id === previous.target ? n.pre.filter((id) => id !== previous.source) : n.pre
      if (n.id === target && !pre.includes(source)) pre = [...pre, source]
      return pre === n.pre ? n : { ...n, pre }
    }),
  }
}
export function createAgentNode(nodes: DagNode[], choice: AgentChoice, source?: DagNode): DagNode {
  const { runner, agent } = choice
  let number = 1
  while (nodes.some((n) => n.name === agent.name + ' ' + number)) number++
  const sameRunner = source && (source.runner === runner.id || source.runner === runner.name),
    sameAgent = sameRunner && source.agent === agent.id
  const parameters = sameAgent
    ? Object.fromEntries(
        Object.entries(source.parameters).filter(([key, value]) => {
          const spec = agent.parameters[key]
          return spec && (!spec.enum?.length || spec.enum.includes(value))
        }),
      )
    : {}
  return {
    id: 'node_' + crypto.randomUUID().replaceAll('-', '').slice(0, 10),
    name: agent.name + ' ' + number,
    pre: source ? [source.id] : [],
    runner: runner.id,
    agent: agent.id,
    cwd: sameRunner ? source.cwd : '',
    prompt: '',
    parameters,
    session: null,
    timeout: sameAgent ? source.timeout : '1h',
  }
}
export function removeNodes(current: GraphDraft, ids: string[]): GraphDraft {
  const positions = Object.fromEntries(
    Object.entries(current.layout.positions).filter(([id]) => !ids.includes(id)),
  )
  return {
    dag: {
      ...current.dag,
      nodes: current.dag.nodes
        .filter((n) => !ids.includes(n.id))
        .map((n) => ({ ...n, pre: n.pre.filter((id) => !ids.includes(id)) })),
    },
    layout: { version: 1, positions },
  }
}
