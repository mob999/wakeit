import { useTranslation } from 'react-i18next'
import { type ReactNode } from 'react'
import { AlarmClock } from './icons'

export function Empty({
  title,
  detail,
  children,
}: {
  title: string
  detail?: string
  children?: ReactNode
}) {
  useTranslation()
  return (
    <div className="empty">
      <AlarmClock size={27} />
      <h3>{title}</h3>
      {detail && <p>{detail}</p>}
      {children}
    </div>
  )
}
