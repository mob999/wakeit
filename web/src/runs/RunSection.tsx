import { useId, type ReactNode } from 'react'
import { ChevronRight } from '../icons'
export function RunSection({
  title,
  open,
  onChange,
  children,
  hidden = false,
  actions,
  embedded = false,
}: {
  title: string
  open: boolean
  onChange: (open: boolean) => void
  children: ReactNode
  hidden?: boolean
  actions?: ReactNode
  embedded?: boolean
}) {
  const id = useId()
  return (
    <section
      className={'run-section ' + (embedded ? 'dag-internal-section' : 'content-card')}
      hidden={hidden}
    >
      <div className={embedded ? 'dag-fold-row' : 'run-section-header card-header'}>
        <button
          type="button"
          className="run-section-heading"
          aria-expanded={open}
          aria-controls={id}
          onClick={() => onChange(!open)}
        >
          <ChevronRight size={14} className={open ? 'expanded' : ''} />
          {title}
        </button>
        {actions && (
          <div className="run-section-actions" hidden={!open}>
            {actions}
          </div>
        )}
      </div>
      <div id={id} className="run-section-body" hidden={!open}>
        {children}
      </div>
    </section>
  )
}
