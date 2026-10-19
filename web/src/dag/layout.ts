import ELK from 'elkjs/lib/elk-api'
// @ts-expect-error Vite emits the worker as a local asset.
import workerUrl from 'elkjs/lib/elk-worker.min.js?url'
import type { DagNode } from '../api'
import type { Point } from './workbench'
export const NODE_WIDTH = 240,
  NODE_HEIGHT = 96
export async function arrangeNodes(nodes: DagNode[]): Promise<Record<string, Point>> {
  if (!nodes.length) return {}
  const elk = new ELK({ workerUrl })
  try {
    const layout = await elk.layout({
      id: 'dag',
      layoutOptions: {
        'elk.algorithm': 'layered',
        'elk.direction': 'RIGHT',
        'elk.edgeRouting': 'ORTHOGONAL',
        'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
        'elk.spacing.nodeNode': '32',
        'elk.layered.spacing.nodeNodeBetweenLayers': '72',
        'elk.padding': '[top=24,left=24,bottom=24,right=24]',
      },
      children: nodes.map((n) => ({ id: n.id, width: NODE_WIDTH, height: NODE_HEIGHT })),
      edges: nodes.flatMap((n) =>
        n.pre
          .filter((id) => nodes.some((p) => p.id === id))
          .map((id) => ({ id: id.length + ':' + id + n.id, sources: [id], targets: [n.id] })),
      ),
    })
    return Object.fromEntries(
      (layout.children ?? []).map((n) => [n.id, { x: n.x ?? 0, y: n.y ?? 0 }]),
    )
  } finally {
    elk.terminateWorker()
  }
}
