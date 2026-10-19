import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import ELK, { type ElkNode } from 'elkjs/lib/elk-api'
// @ts-expect-error Vite emits this worker as a separate local asset.
import workerUrl from 'elkjs/lib/elk-worker.min.js?url'
import { List, GitBranch, Maximize2, Scan, ZoomIn } from 'lucide-react'
import { t } from '../i18n'
import { type DagNode, type DagRunNode, type Agent } from '../api'
import { AgentLogo } from '../AgentLogo'
import { IconButton } from '../ui'
import { Status } from '../Status'
import './dag.css'

export function DagGraph({
  nodes,
  selected,
  onSelect,
  agents = [],
  invalid = [],
  actions,
  compact = false,
}: {
  nodes: (DagNode | DagRunNode)[]
  selected?: string
  onSelect: (id: string) => void
  agents?: Agent[]
  invalid?: string[]
  actions?: React.ReactNode
  compact?: boolean
}) {
  useTranslation()
  const [view, setView] = useState<'graph' | 'list'>('graph')
  const [search, setSearch] = useState('')
  const [layout, setLayout] = useState<ElkNode>()
  const [error, setError] = useState(false)
  const [scale, setScale] = useState(1)
  const viewport = useRef<HTMLDivElement>(null)
  const generation = useRef(0)
  const shape = JSON.stringify(nodes.map((n) => [n.id, n.pre, n.name]))
  useEffect(() => {
    const current = ++generation.current
    const elk = new ELK({ workerUrl })
    setError(false)
    const timer = setTimeout(() => {
      elk
        .layout({
          id: 'dag',
          layoutOptions: {
            'elk.algorithm': 'layered',
            'elk.direction': 'RIGHT',
            'elk.edgeRouting': 'ORTHOGONAL',
            'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
            'elk.spacing.nodeNode': '28',
            'elk.layered.spacing.nodeNodeBetweenLayers': '60',
            'elk.padding': '[top=28,left=28,bottom=28,right=28]',
          },
          children: nodes.map((n) => ({ id: n.id, width: 208, height: 84 })),
          edges: nodes.flatMap((n) =>
            n.pre
              .filter((id) => nodes.some((p) => p.id === id))
              .map((p) => ({ id: `${p}-${n.id}`, sources: [p], targets: [n.id] })),
          ),
        })
        .then((next) => {
          if (current === generation.current) setLayout(next)
        })
        .catch(() => {
          if (current === generation.current) setError(true)
        })
    }, 100)
    return () => {
      clearTimeout(timer)
      elk.terminateWorker()
    }
  }, [shape])
  const visible = nodes.filter((n) =>
    `${n.name} ${n.runner} ${n.agent}`.toLocaleLowerCase().includes(search.toLocaleLowerCase()),
  )
  const nodeLabel = (n: DagNode) => n.name || t('dag.unnamed')
  function select(id: string) {
    onSelect(id)
  }
  const fit = () => {
    const pane = viewport.current
    if (pane && layout?.width && layout.height) {
      setScale(
        Math.min(
          1,
          (pane.clientWidth - 20) / layout.width,
          (pane.clientHeight - 20) / layout.height,
        ),
      )
      pane.scrollTo({ top: 0, left: 0 })
    }
  }
  const related = new Set([
    selected,
    ...(nodes.find((n) => n.id === selected)?.pre ?? []),
    ...nodes.filter((n) => selected && n.pre.includes(selected)).map((n) => n.id),
  ])
  function row(n: DagNode | DagRunNode) {
    const agent = agents.find(
      (a) => a.id === n.agent && (a.runner === n.runner || a.runner_id === n.runner),
    )
    return (
      <>
        <strong>{nodeLabel(n)}</strong>
        <span className="dag-node-meta">
          <span>{n.runner || '—'}</span>
          <span>
            <AgentLogo name={agent?.name ?? n.agent} logo={agent?.logo} />
            {(agent?.name ?? n.agent) || '—'}
          </span>
        </span>
        {'state' in n && (
          <span className="dag-node-state">
            <Status state={n.state} />
            {n.started_at && n.finished_at && (
              <span>
                {Math.max(
                  0,
                  Math.round((Date.parse(n.finished_at) - Date.parse(n.started_at)) / 1000),
                )}
                s
              </span>
            )}
          </span>
        )}
      </>
    )
  }
  return (
    <section
      className={`dag-structure ${compact ? 'dag-compact' : ''}`}
      aria-label={t('dag.nodes')}
    >
      <header className="dag-structure-head">
        <strong>{t('dag.nodes')}</strong>
        <div className="dag-view-toggle">
          <IconButton
            label={t('dag.graph')}
            aria-pressed={view === 'graph'}
            onClick={() => setView('graph')}
          >
            <GitBranch />
          </IconButton>
          <IconButton
            label={t('dag.list')}
            aria-pressed={view === 'list'}
            onClick={() => setView('list')}
          >
            <List />
          </IconButton>
        </div>
        <input
          aria-label={t('dag.search')}
          placeholder={t('dag.search')}
          value={search}
          onChange={(e) => {
            setSearch(e.target.value)
            if (e.target.value) setView('list')
          }}
        />
        {actions}
      </header>
      {view === 'list' || error ? (
        <div className="dag-node-list" role="listbox" aria-label={t('dag.nodes')}>
          {error && (
            <p className="quiet padded" role="status">
              {t('dag.layoutFailed')}
            </p>
          )}
          {visible.map((n, index) => (
            <button
              type="button"
              role="option"
              aria-selected={selected === n.id}
              aria-invalid={invalid.includes(n.id) || undefined}
              key={n.id}
              onClick={() => select(n.id)}
              onKeyDown={(e) => {
                if (['ArrowDown', 'ArrowUp'].includes(e.key)) {
                  e.preventDefault()
                  const next =
                    visible[
                      (index + (e.key === 'ArrowDown' ? 1 : -1) + visible.length) % visible.length
                    ]
                  select(next.id)
                  ;(
                    e.currentTarget.parentElement?.children[
                      (index + (e.key === 'ArrowDown' ? 1 : -1) + visible.length) % visible.length
                    ] as HTMLElement
                  )?.focus()
                }
              }}
              className={`dag-list-node ${selected === n.id ? 'selected' : ''}`}
            >
              {row(n)}
              <small>
                {n.pre.length
                  ? n.pre
                      .map((id) => nodeLabel(nodes.find((p) => p.id === id) ?? { ...n, name: id }))
                      .join(', ')
                  : t('dag.root')}
              </small>
            </button>
          ))}
          {!visible.length && <p className="quiet padded">{t('dag.noNodes')}</p>}
        </div>
      ) : (
        <>
          <div className="dag-graph-scroll" ref={viewport}>
            {!layout && <p className="quiet padded">{t('正在加载…')}</p>}
            {layout && (
              <div
                style={{ width: (layout.width ?? 0) * scale, height: (layout.height ?? 0) * scale }}
              >
                <div
                  className="dag-graph-plane"
                  style={{
                    width: layout.width,
                    height: layout.height,
                    transform: `scale(${scale})`,
                  }}
                >
                  <svg width={layout.width} height={layout.height} aria-hidden="true">
                    <defs>
                      <marker
                        id="dag-arrow"
                        viewBox="0 0 10 10"
                        refX="9"
                        refY="5"
                        markerWidth="5"
                        markerHeight="5"
                        orient="auto-start-reverse"
                      >
                        <path d="M 0 0 L 10 5 L 0 10 z" />
                      </marker>
                    </defs>
                    {layout.edges?.map((edge) => (
                      <g
                        key={edge.id}
                        className={
                          selected &&
                          (edge.sources.includes(selected) || edge.targets.includes(selected))
                            ? 'dag-edge selected'
                            : 'dag-edge'
                        }
                      >
                        {edge.sections?.map((s, i) => (
                          <path
                            key={i}
                            d={[s.startPoint, ...(s.bendPoints ?? []), s.endPoint]
                              .map((p, j) => `${j ? 'L' : 'M'} ${p.x} ${p.y}`)
                              .join(' ')}
                            markerEnd="url(#dag-arrow)"
                          />
                        ))}
                      </g>
                    ))}
                  </svg>
                  {layout.children?.map((position) => {
                    const n = nodes.find((n) => n.id === position.id)
                    if (!n) return null
                    return (
                      <button
                        type="button"
                        key={n.id}
                        className={`dag-graph-node ${selected === n.id ? 'selected' : ''} ${related.has(n.id) ? 'related' : ''}`}
                        style={{
                          left: position.x,
                          top: position.y,
                          width: position.width,
                          height: position.height,
                        }}
                        aria-pressed={selected === n.id}
                        aria-invalid={invalid.includes(n.id) || undefined}
                        aria-label={`${nodeLabel(n)}${'state' in n ? `, ${n.state}` : ''}`}
                        onClick={() => select(n.id)}
                      >
                        {row(n)}
                      </button>
                    )
                  })}
                </div>
              </div>
            )}
          </div>
          <div className="dag-graph-tools">
            <IconButton label={t('dag.fit')} onClick={fit}>
              <Scan />
            </IconButton>
            <IconButton label={t('dag.actualSize')} onClick={() => setScale(1)}>
              <Maximize2 />
            </IconButton>
            <IconButton
              label={t('dag.zoomIn')}
              disabled={scale >= 1.5}
              onClick={() => setScale((s) => Math.min(1.5, s + 0.2))}
            >
              <ZoomIn />
            </IconButton>
          </div>
        </>
      )}
    </section>
  )
}
