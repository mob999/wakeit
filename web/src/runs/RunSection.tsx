import { useId, type ReactNode } from 'react'
import { ChevronRight } from '../components/icons'
export function RunSection({
  title,
  open,
  onChange,
  children,
  hidden = false,
  actions,
  embedded = false,
  collapsible = true,
}: {
  title: string
  children: ReactNode
  hidden?: boolean
  actions?: ReactNode
  embedded?: boolean
} & (
  | { collapsible?: true; open: boolean; onChange: (open: boolean) => void }
  | { collapsible: false; open?: never; onChange?: never }
)) {
  const id = useId()
  const visible = !collapsible || open
  return (
    <section
      className={'run-section ' + (embedded ? 'dag-internal-section' : 'content-card')}
      hidden={hidden}
    >
      <div className={embedded ? 'dag-fold-row' : 'run-section-header card-header'}>
        {collapsible ? (
          <button
            type="button"
            className="run-section-heading"
            aria-expanded={open}
            aria-controls={id}
            onClick={() => onChange?.(!open)}
          >
            <ChevronRight size={14} className={open ? 'expanded' : ''} />
            {title}
          </button>
        ) : (
          <h2 className="run-section-heading">{title}</h2>
        )}
        {actions && (
          <div className="run-section-actions" hidden={!visible}>
            {actions}
          </div>
        )}
      </div>
      <div id={id} className="run-section-body" hidden={!visible}>
        {children}
      </div>
    </section>
  )
}
