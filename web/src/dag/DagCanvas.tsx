import { ActionIcon } from '../actions/ActionIcon'
import {
  forwardRef,
  memo,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import {
  ReactFlow,
  ReactFlowProvider,
  Handle,
  Position,
  applyNodeChanges,
  applyEdgeChanges,
  useReactFlow,
  type Node,
  type Edge,
  type NodeProps,
  type Connection,
  type NodeChange,
  type EdgeChange,
  type FinalConnectionState,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'

import type { Agent, DagNode, EditorLayout, Runner } from '../api/types'
import { IconButton } from '../components/ui'
import { t } from '../i18n'
import { nodeName, runnerName } from './identity'
import { arrangeNodes, NODE_HEIGHT, NODE_WIDTH } from './layout'
import { positionOf, type Point } from './workbench'
import { NodeCardContent } from './NodeCard'
export type CanvasHandle = {
  point: (screen: Point) => Point
  center: () => Point
  fit: () => void
  zoom: (delta: number) => void
  locate: (id: string) => void
}
type Data = {
  config: DagNode
  runner: string
  agent?: Agent
  output: boolean
  invalid: boolean
  matched: boolean
  disabled: boolean
  onAdd: () => void
  actions: ReactNode
}
type CanvasNode = Node<Data, 'agent'>
const AgentNode = memo(function AgentNode({ data, selected }: NodeProps<CanvasNode>) {
  return (
    <div
      className={
        'dag-canvas-node dag-node-card ' +
        (selected ? 'selected ' : '') +
        (data.invalid ? 'invalid ' : '') +
        (data.matched ? 'matched' : '')
      }
      data-node={data.config.id}
      aria-description={data.output ? t('dag.outputNode') : undefined}
    >
      <Handle type="target" position={Position.Left} aria-label={t('dag.inputPort')} />
      <NodeCardContent
        name={nodeName(data.config)}
        runner={data.runner}
        agent={data.agent?.name ?? data.config.agent}
        logo={data.agent?.logo}
        output={data.output}
        invalid={data.invalid}
        actions={data.actions}
      />
      <div className="dag-canvas-node-add nodrag nopan">
        <IconButton
          label={t('dag.addNext')}
          disabled={data.disabled}
          onClick={(e) => {
            e.stopPropagation()
            data.onAdd()
          }}
        >
          <ActionIcon name="add" />
        </IconButton>
      </div>
      <Handle type="source" position={Position.Right} aria-label={t('dag.outputPort')} />
    </div>
  )
})
const nodeTypes = { agent: AgentNode }
const edgeId = (source: string, target: string) =>
  'edge-' + source.length + '-' + source + '-' + target
export type CanvasProps = {
  nodes: DagNode[]
  layout: EditorLayout
  runners: Runner[]
  agents: Agent[]
  selected: string[]
  outputs: string[]
  invalid: string[]
  search: string
  disabled: boolean
  active: boolean
  panelWidth: number
  actions: (node: DagNode) => ReactNode
  onSelect: (ids: string[]) => void
  onOpen: (id: string) => void
  onBlank: () => void
  onPositions: (positions: Record<string, Point>) => void
  onInitialize: (positions: Record<string, Point>) => void
  onConnect: (source: string, target: string, previous?: { source: string; target: string }) => void
  valid: (source: string, target: string, previous?: { source: string; target: string }) => boolean
  onDelete: (nodes: string[], edges: { source: string; target: string }[]) => void
  onAdd: (source: string, position: Point) => void
  onLayoutError: () => void
}
const CanvasInner = forwardRef<CanvasHandle, CanvasProps>(function CanvasInner(props, ref) {
  const callbacks = useRef(props)
  callbacks.current = props
  const flow = useReactFlow<CanvasNode>(),
    host = useRef<HTMLDivElement>(null)
  const [nodes, setNodes] = useState<CanvasNode[]>([]),
    [edges, setEdges] = useState<Edge[]>([]),
    [ready, setReady] = useState(false)
  const dragging = useRef(false),
    reconnecting = useRef<Edge | null>(null),
    fitted = useRef(false)
  const point = useCallback(
    (screen: Point) => {
      const p = flow.screenToFlowPosition(screen)
      return { x: p.x - NODE_WIDTH / 2, y: p.y - NODE_HEIGHT / 2 }
    },
    [flow],
  )
  const locate = useCallback(
    (id: string) => {
      const node = flow.getNode(id),
        element = host.current
      if (!node || !element) return
      const box = element.getBoundingClientRect(),
        viewport = flow.getViewport(),
        zoom = viewport.zoom
      const left = box.left + viewport.x + node.position.x * zoom,
        top = box.top + viewport.y + node.position.y * zoom
      const right =
          box.right - callbacks.current.panelWidth - (callbacks.current.panelWidth ? 32 : 16),
        bottom = box.bottom - 16
      let dx = 0,
        dy = 0
      if (left < box.left + 16) dx = box.left + 16 - left
      else if (left + NODE_WIDTH * zoom > right) dx = right - left - NODE_WIDTH * zoom
      if (top < box.top + 16) dy = box.top + 16 - top
      else if (top + NODE_HEIGHT * zoom > bottom) dy = bottom - top - NODE_HEIGHT * zoom
      if (dx || dy) void flow.setViewport({ ...viewport, x: viewport.x + dx, y: viewport.y + dy })
    },
    [flow],
  )
  useImperativeHandle(
    ref,
    () => ({
      point,
      center: () => {
        const b = host.current!.getBoundingClientRect()
        return point({
          x: b.left + (b.width - callbacks.current.panelWidth) / 2,
          y: b.top + b.height / 2,
        })
      },
      fit: () => {
        void flow.fitView({ padding: 0.15, maxZoom: 1, minZoom: 0.25 })
      },
      zoom: (delta) => {
        void flow.zoomTo(Math.max(0.25, Math.min(2, flow.getZoom() + delta)))
      },
      locate,
    }),
    [flow, point, locate],
  )
  useEffect(() => {
    let live = true
    const initial = callbacks.current.nodes.filter(
      (n) => !positionOf(callbacks.current.layout, n.id),
    )
    if (!initial.length) {
      setReady(true)
      return
    }
    arrangeNodes(callbacks.current.nodes)
      .then((positions) => {
        if (live) {
          callbacks.current.onInitialize(
            Object.fromEntries(initial.map((n) => [n.id, positions[n.id]])),
          )
          setReady(true)
        }
      })
      .catch(() => {
        if (live) {
          callbacks.current.onLayoutError()
          callbacks.current.onInitialize(
            Object.fromEntries(
              initial.map((n, i) => [
                n.id,
                { x: 40 + (i % 4) * 312, y: 40 + Math.floor(i / 4) * 128 },
              ]),
            ),
          )
          setReady(true)
        }
      })
    return () => {
      live = false
    }
  }, [])
  useEffect(() => {
    const query = props.search.toLowerCase().trim()
    setNodes((old) =>
      props.nodes.map((config, i) => {
        const previous = old.find((node) => node.id === config.id)
        return {
          id: config.id,
          type: 'agent',
          position: previous?.dragging
            ? previous.position
            : (positionOf(props.layout, config.id) ?? { x: 40 + i * 312, y: 40 }),
          selected: props.selected.includes(config.id),
          width: NODE_WIDTH,
          height: NODE_HEIGHT,
          ariaLabel: nodeName(config),
          domAttributes: {
            'aria-description': props.outputs.includes(config.id) ? t('dag.outputNode') : undefined,
          },
          data: {
            config,
            runner: runnerName(config, props.runners, props.agents),
            agent: props.agents.find(
              (a) =>
                a.id === config.agent &&
                (a.runner_id === config.runner || a.runner === config.runner),
            ),
            output: props.outputs.includes(config.id),
            invalid: props.invalid.includes(config.id),
            matched: !!query && (nodeName(config) + ' ' + config.id).toLowerCase().includes(query),
            disabled: props.disabled,
            actions: props.actions(config),
            onAdd: () => {
              const position = flow.getNode(config.id)?.position ??
                positionOf(props.layout, config.id) ?? { x: 0, y: 0 }
              callbacks.current.onAdd(config.id, { x: position.x + NODE_WIDTH + 72, y: position.y })
            },
          },
          dragging: previous?.dragging,
        }
      }),
    )
  }, [
    props.nodes,
    props.layout,
    props.selected,
    props.outputs.join(','),
    props.invalid.join(','),
    props.search,
    props.runners,
    props.agents,
    props.disabled,
    props.actions,
    flow,
  ])
  const shape = JSON.stringify(props.nodes.map((n) => [n.id, n.pre]))
  useEffect(() => {
    setEdges((old) =>
      props.nodes.flatMap((n) =>
        n.pre
          .filter((id) => props.nodes.some((source) => source.id === id))
          .map((source) => ({
            id: edgeId(source, n.id),
            source,
            target: n.id,
            type: 'smoothstep',
            selected: old.find((e) => e.id === edgeId(source, n.id))?.selected,
            markerEnd: { type: 'arrowclosed' as const, width: 16, height: 16 },
            ariaLabel: nodeName(props.nodes.find((p) => p.id === source)!) + ' → ' + nodeName(n),
            reconnectable: !props.disabled,
          })),
      ),
    )
  }, [shape, props.disabled])
  useEffect(() => {
    if (ready && props.active && !fitted.current && host.current?.clientWidth && nodes.length) {
      fitted.current = true
      requestAnimationFrame(() => {
        void flow.fitView({ padding: 0.2, maxZoom: 1, minZoom: 0.65 })
      })
    }
  }, [ready, props.active, nodes.length, flow])
  const firstSelected = props.selected[0]
  useEffect(() => {
    if (props.active && firstSelected && props.panelWidth)
      requestAnimationFrame(() => locate(firstSelected))
  }, [firstSelected, props.panelWidth, props.active, locate])
  const selection = useCallback(
    ({ nodes }: { nodes: CanvasNode[] }) => callbacks.current.onSelect(nodes.map((n) => n.id)),
    [],
  )
  const nodeChanges = useCallback((changes: NodeChange<CanvasNode>[]) => {
    setNodes((old) => applyNodeChanges(changes, old))
    const positions = changes.flatMap((c) =>
      c.type === 'position' && c.position && c.dragging === false
        ? [[c.id, c.position] as const]
        : [],
    )
    if (!dragging.current && positions.length && !callbacks.current.disabled)
      callbacks.current.onPositions(Object.fromEntries(positions))
  }, [])
  const edgeChanges = useCallback(
    (changes: EdgeChange[]) => setEdges((old) => applyEdgeChanges(changes, old)),
    [],
  )
  function finishConnection(_event: MouseEvent | TouchEvent, state: FinalConnectionState) {
    if (reconnecting.current || !state.fromNode || !state.fromHandle || callbacks.current.disabled)
      return
    if (state.toNode && state.isValid === false) {
      const source = state.fromHandle.type === 'source' ? state.fromNode.id : state.toNode.id,
        target = state.fromHandle.type === 'source' ? state.toNode.id : state.fromNode.id
      callbacks.current.onConnect(source, target)
      return
    }
    if (!state.toNode && state.fromHandle.type === 'source') {
      const e = 'changedTouches' in _event ? _event.changedTouches[0] : _event
      const b = host.current!.getBoundingClientRect()
      if (
        e.clientX >= b.left &&
        e.clientX <= b.right &&
        e.clientY >= b.top &&
        e.clientY <= b.bottom &&
        !document.elementFromPoint(e.clientX, e.clientY)?.closest('.dag-floating-panel')
      )
        callbacks.current.onAdd(state.fromNode.id, point({ x: e.clientX, y: e.clientY }))
    }
  }
  return (
    <div className="dag-interactive-canvas" data-layout-ready={ready} ref={host}>
      <ReactFlow<CanvasNode>
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        onNodesChange={nodeChanges}
        onEdgesChange={edgeChanges}
        onSelectionChange={selection}
        onNodeClick={(event, node) => {
          if (!event.shiftKey) callbacks.current.onOpen(node.id)
        }}
        onPaneClick={() => callbacks.current.onBlank()}
        onConnect={(c: Connection) => callbacks.current.onConnect(c.source, c.target)}
        isValidConnection={(c) =>
          callbacks.current.valid(c.source!, c.target!, reconnecting.current ?? undefined)
        }
        onConnectEnd={finishConnection}
        onReconnectStart={(_, edge) => {
          reconnecting.current = edge
        }}
        onReconnect={(edge, c) => callbacks.current.onConnect(c.source, c.target, edge)}
        onReconnectEnd={(_, edge, handle, state) => {
          if (state.toNode && !state.isValid && state.fromNode) {
            const source = handle === 'target' ? edge.source : state.toNode.id
            const target = handle === 'source' ? edge.target : state.toNode.id
            callbacks.current.onConnect(source, target, edge)
          }
          setTimeout(() => {
            reconnecting.current = null
          }, 0)
        }}
        onNodeDragStart={() => {
          dragging.current = true
        }}
        onNodeDragStop={(_, _node, dragged) => {
          dragging.current = false
          callbacks.current.onPositions(Object.fromEntries(dragged.map((n) => [n.id, n.position])))
        }}
        onBeforeDelete={async ({ nodes, edges }) => {
          if (!callbacks.current.disabled)
            callbacks.current.onDelete(
              nodes.map((n) => n.id),
              edges.map((e) => ({ source: e.source, target: e.target })),
            )
          return false
        }}
        nodesDraggable={!props.disabled}
        nodesConnectable={!props.disabled}
        elementsSelectable={!props.disabled}
        edgesReconnectable={!props.disabled}
        deleteKeyCode={props.disabled ? null : ['Backspace', 'Delete']}
        panOnScroll
        selectionOnDrag={false}
        panOnDrag={[0, 1, 2]}
        zoomOnScroll={false}
        zoomOnPinch
        zoomActivationKeyCode="Meta"
        selectionKeyCode="Shift"
        multiSelectionKeyCode="Shift"
        minZoom={0.25}
        maxZoom={2}
        proOptions={{ hideAttribution: true }}
      ></ReactFlow>
      {!props.nodes.length && <div className="dag-canvas-empty">{t('dag.dropAgent')}</div>}
    </div>
  )
})
export const DagCanvas = forwardRef<CanvasHandle, CanvasProps>(function DagCanvas(props, ref) {
  return (
    <ReactFlowProvider>
      <CanvasInner {...props} ref={ref} />
    </ReactFlowProvider>
  )
})
