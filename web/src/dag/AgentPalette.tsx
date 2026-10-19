import { useRef, useState, type PointerEvent } from 'react'
import { ChevronDown, Search } from 'lucide-react'
import type { Runner } from '../api'
import { t } from '../i18n'
import { AgentLogo } from '../AgentLogo'
import type { AgentChoice, Point } from './workbench'
export function AgentPalette({
  runners,
  disabled,
  onChoose,
  onDrag,
  compact = false,
}: {
  runners: Runner[]
  disabled: boolean
  compact?: boolean
  onChoose: (choice: AgentChoice) => void
  onDrag: (choice: AgentChoice, point: Point, phase: 'move' | 'drop' | 'cancel') => void
}) {
  const [search, setSearch] = useState(''),
    [closed, setClosed] = useState<string[]>([])
  const gesture = useRef<{ choice: AgentChoice; x: number; y: number; moved: boolean } | null>(null)
  const query = search.trim().toLowerCase()
  const visible = runners
    .filter((r) => !r.revoked)
    .map((r) => ({
      ...r,
      agents: r.agents.filter(
        (a) =>
          a.available &&
          (r.name + ' ' + r.id + ' ' + a.name + ' ' + a.id).toLowerCase().includes(query),
      ),
    }))
    .filter((r) => r.agents.length)
  function pointer(event: PointerEvent<HTMLButtonElement>, phase: 'move' | 'drop' | 'cancel') {
    const current = gesture.current
    if (!current) return
    const point = { x: event.clientX, y: event.clientY }
    if (Math.hypot(point.x - current.x, point.y - current.y) > 5) current.moved = true
    if (phase === 'move' && current.moved) onDrag(current.choice, point, phase)
    if (phase !== 'move') {
      gesture.current = null
      if (phase === 'cancel') onDrag(current.choice, point, phase)
      else if (current.moved) onDrag(current.choice, point, phase)
      else if (!disabled) onChoose(current.choice)
    }
  }
  return (
    <div className={'dag-agent-palette ' + (compact ? 'compact' : '')}>
      <div className="dag-palette-search">
        <Search size={14} />
        <input
          aria-label={t('dag.searchAgents')}
          placeholder={t('搜索…')}
          value={search}
          disabled={disabled}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>
      <div className="dag-palette-groups">
        {visible.map((r) => (
          <section key={r.id}>
            <button
              type="button"
              className="dag-palette-runner"
              disabled={disabled}
              aria-expanded={!closed.includes(r.id) || !!query}
              onClick={() =>
                setClosed((old) =>
                  old.includes(r.id) ? old.filter((id) => id !== r.id) : [...old, r.id],
                )
              }
            >
              <ChevronDown
                size={12}
                className={closed.includes(r.id) && !query ? 'collapsed' : ''}
              />
              <span title={r.name}>{r.name}</span>
              <span
                className={'dag-device-dot ' + (r.online ? 'online' : '')}
                aria-label={t(r.online ? '在线' : '离线')}
              />
              {!r.online && <span className="quiet">{t('离线')}</span>}
            </button>
            {(!closed.includes(r.id) || !!query) &&
              r.agents.map((agent) => (
                <button
                  type="button"
                  key={agent.id}
                  className="dag-agent-entry"
                  disabled={disabled}
                  aria-label={t('dag.agentOnDevice', { agent: agent.name, runner: r.name })}
                  onPointerDown={(event) => {
                    if (disabled || event.button !== 0) return
                    event.currentTarget.setPointerCapture(event.pointerId)
                    gesture.current = {
                      choice: { runner: r, agent },
                      x: event.clientX,
                      y: event.clientY,
                      moved: false,
                    }
                  }}
                  onPointerMove={(e) => pointer(e, 'move')}
                  onPointerUp={(e) => pointer(e, 'drop')}
                  onPointerCancel={(e) => pointer(e, 'cancel')}
                  onClick={(e) => {
                    if (e.detail === 0 && !disabled) onChoose({ runner: r, agent })
                  }}
                >
                  <AgentLogo name={agent.name} logo={agent.logo} />
                  <span>{agent.name}</span>
                </button>
              ))}
          </section>
        ))}
        {!visible.length && <p className="quiet card-state">{t('dag.noAgents')}</p>}
      </div>
    </div>
  )
}
