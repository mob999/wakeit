import { ErrorToast } from '../Toast'
import { useEffect, useLayoutEffect, useRef, useState, useId, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import ELK, { type ElkNode } from 'elkjs/lib/elk-api'
// @ts-expect-error Vite emits this worker as a separate local asset.
import workerUrl from 'elkjs/lib/elk-worker.min.js?url'
import { List, GitBranch, CircleAlert, ChevronRight } from 'lucide-react'
import { t } from '../i18n'
import { type DagNode, type DagRunNode, type Agent, type Runner } from '../api'
import { IconButton } from '../ui'
import { AgentLogo } from '../AgentLogo'
import { Status } from '../Status'
import { nodeName, runnerName } from './identity'
import { CanvasControls } from './CanvasControls'
import { NodeCardContent, OutputBadge } from './NodeCard'
import { NODE_HEIGHT, NODE_WIDTH } from './layout'
import './dag.css'
export type DagView = 'graph' | 'list'
export function DagGraph({
  nodes,
  selected,
  onSelect,
  agents = [],
  runners = [],
  invalid = [],
  actions,
  compact = false,
  view: controlledView,
  onViewChange,
  search: controlledSearch,
  onSearchChange,
  hideHeader = false,
  hideTitle = false,
  outputIds = [],
  inlineNodeId,
  inline,
  rowActions,
  disabled = false,
  locateRequest,
}: {
  nodes: (DagNode | DagRunNode)[]
  selected?: string
  onSelect: (id: string) => void
  agents?: Agent[]
  runners?: Runner[]
  invalid?: string[]
  actions?: ReactNode
  compact?: boolean
  view?: DagView
  onViewChange?: (view: DagView) => void
  search?: string
  onSearchChange?: (value: string) => void
  hideHeader?: boolean
  outputIds?: string[]
  hideTitle?: boolean
  inlineNodeId?: string | null
  inline?: ReactNode
  rowActions?: (node: DagNode) => ReactNode
  disabled?: boolean
  locateRequest?: { id: string; sequence: number }
}) {
  useTranslation()
  const markerId = useId().replaceAll(':', '')
  const scroll = useRef({ left: 0, top: 0 })
  const structure = useRef<HTMLElement>(null),
    originalHeight = useRef(0)
  const [fullscreen, setFullscreen] = useState(false)
  function toggleFullscreen() {
    if (!fullscreen) originalHeight.current = structure.current?.getBoundingClientRect().height ?? 0
    setFullscreen((value) => !value)
  }
  const [localView, setView] = useState<DagView>('graph'),
    [localSearch, setSearch] = useState('')
  const view = controlledView ?? localView,
    search = controlledSearch ?? localSearch
  const changeView = (next: DagView) => (onViewChange ? onViewChange(next) : setView(next))
  const [layout, setLayout] = useState<ElkNode>(),
    [error, setError] = useState(false),
    [scale, setScale] = useState(1)
  useEffect(() => {
    if (view !== 'graph' || error) setFullscreen(false)
  }, [view, error])
  useEffect(() => {
    if (!fullscreen) return
    const navigation = document.getElementById('primary-navigation'),
      previous = navigation?.inert
    if (navigation) navigation.inert = true
    function escape(event: KeyboardEvent) {
      if (
        event.key !== 'Escape' ||
        event.defaultPrevented ||
        document.querySelector(
          '[data-radix-popper-content-wrapper], [role=dialog][data-state=open]',
        )
      )
        return
      event.preventDefault()
      setFullscreen(false)
    }
    window.addEventListener('keydown', escape)
    return () => {
      if (navigation) navigation.inert = previous ?? false
      window.removeEventListener('keydown', escape)
    }
  }, [fullscreen])
  const [size, setSize] = useState({ width: 0, height: 0 })
  const viewport = useRef<HTMLDivElement>(null),
    fitted = useRef(false),
    generation = useRef(0),
    previous = useRef(selected)
  useLayoutEffect(() => {
    viewport.current?.scrollTo(scroll.current.left, scroll.current.top)
  }, [controlledView, localView])
  const shape = JSON.stringify(nodes.map((n) => [n.id, n.pre]))
  useEffect(() => {
    const current = ++generation.current,
      elk = new ELK({ workerUrl })
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
            'elk.spacing.nodeNode': '24',
            'elk.layered.spacing.nodeNodeBetweenLayers': '48',
            'elk.padding': '[top=20,left=20,bottom=20,right=20]',
          },
          children: nodes.map((n) => ({ id: n.id, width: NODE_WIDTH, height: NODE_HEIGHT })),
          edges: nodes.flatMap((n) =>
            n.pre
              .filter((id) => nodes.some((p) => p.id === id))
              .map((p) => ({ id: p + '-' + n.id, sources: [p], targets: [n.id] })),
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
  useEffect(() => {
    if (!viewport.current) return
    const observer = new ResizeObserver(([entry]) => {
      if (entry.contentRect.width)
        setSize({ width: entry.contentRect.width, height: entry.contentRect.height })
    })
    observer.observe(viewport.current)
    return () => observer.disconnect()
  }, [view, error])
  const offset = {
    x: Math.max(0, (size.width - (layout?.width ?? 0) * scale) / 2),
    y: Math.max(0, (size.height - (layout?.height ?? 0) * scale) / 2),
  }
  function locate(id = selected) {
    const position = layout?.children?.find((n) => n.id === id),
      pane = viewport.current
    if (position && pane)
      pane.scrollTo({
        left:
          offset.x + ((position.x ?? 0) + (position.width ?? 0) / 2) * scale - pane.clientWidth / 2,
        top:
          offset.y +
          ((position.y ?? 0) + (position.height ?? 0) / 2) * scale -
          pane.clientHeight / 2,
      })
  }
  useEffect(() => {
    if (!layout || !size.width || fitted.current) return
    setScale(
      Math.max(
        0.75,
        Math.min(1, size.width / (layout.width ?? 1), size.height / (layout.height ?? 1)),
      ),
    )
    fitted.current = true
  }, [layout, size])
  useEffect(() => {
    if (
      viewport.current &&
      selected &&
      selected !== previous.current &&
      layout?.children?.some((n) => n.id === selected)
    ) {
      locate(selected)
      previous.current = selected
    }
  }, [selected, layout, view, error])
  useEffect(() => {
    if (
      locateRequest &&
      viewport.current &&
      layout?.children?.some((n) => n.id === locateRequest.id)
    )
      locate(locateRequest.id)
  }, [locateRequest, layout, view])
  const matches = (n: DagNode) =>
    (nodeName(n) + ' ' + n.id + ' ' + runnerName(n, runners, agents) + ' ' + n.agent)
      .toLowerCase()
      .includes(search.toLowerCase().trim())
  const visible = nodes.filter(matches),
    actualView = error ? 'list' : view
  function flags(n: DagNode) {
    return (
      <>
        {outputIds.includes(n.id) && <OutputBadge />}
        {invalid.includes(n.id) && <CircleAlert size={13} aria-label={t('dag.checkNode')} />}
      </>
    )
  }
  const agentOf = (n: DagNode) =>
    agents.find((a) => a.id === n.agent && (a.runner === n.runner || a.runner_id === n.runner))
  function metadata(n: DagNode, table = false) {
    const agent = agentOf(n)
    return (
      <>
        <span role={table ? 'cell' : undefined} title={runnerName(n, runners, agents)}>
          {runnerName(n, runners, agents)}
        </span>
        <span role={table ? 'cell' : undefined} className="run-agent">
          {n.agent && <AgentLogo name={agent?.name ?? n.agent} logo={agent?.logo} />}
          {agent?.name || n.agent || '—'}
        </span>
      </>
    )
  }
  return (
    <section
      className={'dag-structure ' + (compact ? 'dag-compact' : '')}
      ref={structure}
      style={fullscreen ? { height: originalHeight.current } : undefined}
      aria-label={t('dag.nodes')}
      data-view={actualView}
    >
      <div className={'dag-structure-content' + (fullscreen ? ' dag-structure-fullscreen' : '')}>
        {!hideHeader && (
          <header className="dag-structure-head">
            {!hideTitle && <strong>{t('dag.nodes')}</strong>}
            <IconButton
              label={t('dag.graph')}
              aria-pressed={view === 'graph'}
              onClick={() => changeView('graph')}
            >
              <GitBranch />
            </IconButton>
            <IconButton
              label={t('dag.list')}
              aria-pressed={view === 'list'}
              onClick={() => changeView('list')}
            >
              <List />
            </IconButton>
            <input
              aria-label={t('dag.search')}
              placeholder={t('dag.search')}
              value={search}
              onChange={(e) =>
                onSearchChange ? onSearchChange(e.target.value) : setSearch(e.target.value)
              }
            />
            {actions}
          </header>
        )}
        {error && <ErrorToast error={t('dag.layoutFailed')} />}
        {actualView === 'list' ? (
          <div className="dag-node-table" role="table" aria-label={t('dag.nodes')}>
            <div className="dag-node-columns dag-table-head" role="row">
              {[t('dag.nodes'), t('dag.pre'), t('Runner'), t('Agent'), t('操作')].map((label) => (
                <span key={label} role="columnheader">
                  {label}
                </span>
              ))}
            </div>
            {visible.map((n) => (
              <div className="dag-table-entry" key={n.id}>
                <div
                  role="row"
                  tabIndex={disabled ? -1 : 0}
                  aria-selected={selected === n.id}
                  aria-label={nodeName(n)}
                  aria-invalid={invalid.includes(n.id) || undefined}
                  className={
                    'dag-list-node dag-node-columns ' + (selected === n.id ? 'selected' : '')
                  }
                  onClick={() => !disabled && onSelect(n.id)}
                  onKeyDown={(e) => {
                    if (e.target !== e.currentTarget) return
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault()
                      if (!disabled) onSelect(n.id)
                    }
                    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
                      e.preventDefault()
                      const index = visible.indexOf(n),
                        next =
                          visible[
                            (index + (e.key === 'ArrowDown' ? 1 : -1) + visible.length) %
                              visible.length
                          ]
                      e.currentTarget
                        .closest('[role=table]')
                        ?.querySelector<HTMLElement>('[data-node="' + next.id + '"]')
                        ?.focus()
                    }
                  }}
                  data-node={n.id}
                >
                  <span role="cell" className="dag-node-title">
                    <strong>{nodeName(n)}</strong>
                    {flags(n)}
                    {'state' in n && <Status state={n.state} />}
                  </span>
                  <span
                    role="cell"
                    className="dag-pre-summary"
                    title={n.pre
                      .map((id) => nodeName(nodes.find((p) => p.id === id) ?? { name: id }))
                      .join(', ')}
                  >
                    {n.pre.length
                      ? n.pre
                          .map((id) => nodeName(nodes.find((p) => p.id === id) ?? { name: id }))
                          .join(', ')
                      : t('dag.root')}
                  </span>
                  {metadata(n, true)}
                  <span
                    role="cell"
                    className="dag-row-actions"
                    onClick={(e) => e.stopPropagation()}
                  >
                    {rowActions ? rowActions(n) : <ChevronRight size={14} />}
                  </span>
                </div>
                {inlineNodeId === n.id && <div className="dag-inline-node">{inline}</div>}
              </div>
            ))}
            {!visible.length && <p className="card-state quiet">{t('dag.noMatches')}</p>}
          </div>
        ) : (
          <>
            <div className="dag-graph-frame">
              <div
                className="dag-graph-scroll"
                ref={viewport}
                onScroll={(e) => {
                  scroll.current = {
                    left: e.currentTarget.scrollLeft,
                    top: e.currentTarget.scrollTop,
                  }
                }}
              >
                {!layout && <p className="quiet padded">{t('正在加载…')}</p>}
                {layout && (
                  <div
                    className="dag-graph-surface"
                    style={{
                      width: Math.max(size.width, (layout.width ?? 0) * scale),
                      height: Math.max(size.height, (layout.height ?? 0) * scale),
                    }}
                  >
                    <div
                      className="dag-graph-plane"
                      style={{
                        left: offset.x,
                        top: offset.y,
                        width: layout.width,
                        height: layout.height,
                        transform: 'scale(' + scale + ')',
                      }}
                    >
                      <svg width={layout.width} height={layout.height} aria-hidden="true">
                        <defs>
                          <marker
                            id={'dag-arrow-' + markerId}
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
                              'dag-edge ' +
                              (edge.sources.includes(selected ?? '') ||
                              edge.targets.includes(selected ?? '')
                                ? 'selected'
                                : '')
                            }
                          >
                            {edge.sections?.map((section, i) => (
                              <path
                                key={i}
                                markerEnd={'url(#dag-arrow-' + markerId + ')'}
                                d={[
                                  section.startPoint,
                                  ...(section.bendPoints ?? []),
                                  section.endPoint,
                                ]
                                  .map((p, i) => (i ? 'L ' : 'M ') + p.x + ' ' + p.y)
                                  .join(' ')}
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
                            disabled={disabled}
                            className={
                              'dag-graph-node dag-node-card ' +
                              (selected === n.id ? 'selected ' : '') +
                              (search && matches(n) ? 'matched' : '')
                            }
                            style={{
                              left: position.x,
                              top: position.y,
                              width: position.width,
                              height: position.height,
                            }}
                            aria-pressed={selected === n.id}
                            aria-invalid={invalid.includes(n.id) || undefined}
                            aria-label={nodeName(n)}
                            aria-description={
                              outputIds.includes(n.id) ? t('dag.outputNode') : undefined
                            }
                            onClick={() => onSelect(n.id)}
                          >
                            <NodeCardContent
                              name={nodeName(n)}
                              runner={runnerName(n, runners, agents)}
                              agent={agentOf(n)?.name || n.agent || '—'}
                              logo={agentOf(n)?.logo}
                              output={outputIds.includes(n.id)}
                              invalid={invalid.includes(n.id)}
                              status={'state' in n ? <Status state={n.state} /> : undefined}
                            />
                          </button>
                        )
                      })}
                    </div>
                  </div>
                )}
              </div>
              <CanvasControls
                fullscreen={fullscreen}
                disabled={disabled}
                onZoom={(direction) =>
                  setScale((value) => Math.max(0.1, Math.min(2, value + direction * 0.1)))
                }
                onToggleFullscreen={toggleFullscreen}
              />
            </div>
            {inline}
          </>
        )}
      </div>
    </section>
  )
}
