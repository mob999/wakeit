import { type ReactNode } from 'react'
export function ContentCard({
  title,
  actions,
  children,
  className = '',
}: {
  title: string
  actions?: ReactNode
  children: ReactNode
  className?: string
}) {
  return (
    <section className={`content-card ${className}`}>
      <div className="card-header">
        <h3>{title}</h3>
        {actions && <div className="actions flex items-center gap-[7px]">{actions}</div>}
      </div>
      <div className="card-body">{children}</div>
    </section>
  )
}
