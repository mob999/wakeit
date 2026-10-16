import { t as tr } from './i18n'
import { useTranslation } from 'react-i18next'
import {
  createContext,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { createPortal } from 'react-dom'
import { MoreHorizontal } from 'lucide-react'
import { IconButton } from './ui'

export const PageHeaderContext = createContext<{
  breadcrumbs: HTMLDivElement | null
  actions: HTMLDivElement | null
}>({ breadcrumbs: null, actions: null })

export const PageListViewContext = createContext(false)

export type Breadcrumb = { label: string; onClick?: () => void }
export function PageBreadcrumbs({
  label,
  crumbs = [],
  onBack,
}: {
  label: string
  crumbs?: Breadcrumb[]
  onBack?: () => void
}) {
  useTranslation()
  const { breadcrumbs } = useContext(PageHeaderContext)
  const listView = useContext(PageListViewContext)
  if (!breadcrumbs) return null
  return createPortal(
    <nav className="breadcrumbs" aria-label={tr('层级导航')} data-list-view={listView || undefined}>
      <ol>
        <li>
          {crumbs.length && onBack ? (
            <button onClick={onBack}>{label}</button>
          ) : (
            <span aria-current="page" role="heading" aria-level={1}>
              {label}
            </span>
          )}
        </li>
        {crumbs.map((crumb, index) => (
          <li key={`${index}-${crumb.label}`}>
            <span className="breadcrumb-separator" aria-hidden="true">
              /
            </span>
            <span
              className="breadcrumb-label"
              role={index === 0 ? 'heading' : undefined}
              aria-level={index === 0 ? 1 : undefined}
            >
              {index < crumbs.length - 1 && crumb.onClick ? (
                <button onClick={crumb.onClick} title={crumb.label}>
                  {crumb.label}
                </button>
              ) : (
                <span
                  aria-current={index === crumbs.length - 1 ? 'page' : undefined}
                  title={crumb.label}
                >
                  {crumb.label}
                </span>
              )}
            </span>
          </li>
        ))}
      </ol>
    </nav>,
    breadcrumbs,
  )
}

export function PageActions({ children }: { children?: ReactNode }) {
  useTranslation()
  const { actions } = useContext(PageHeaderContext)
  const listView = useContext(PageListViewContext)
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  const trigger = useRef<HTMLButtonElement>(null)
  const id = useId()
  useEffect(() => {
    setOpen(false)
  }, [listView])
  useEffect(() => {
    if (!open) return
    const outside = (event: PointerEvent) => {
      if (!ref.current?.contains(event.target as Node)) setOpen(false)
    }
    document.addEventListener('pointerdown', outside)
    return () => document.removeEventListener('pointerdown', outside)
  }, [open])
  if (!actions || !children) return null
  return createPortal(
    <div
      className={`page-actions ${listView ? 'list-view-on-narrow' : ''}`}
      ref={ref}
      onKeyDown={(event) => {
        if (event.key === 'Escape' && open && ref.current?.contains(event.target as Node)) {
          event.stopPropagation()
          setOpen(false)
          trigger.current?.focus()
        }
      }}
    >
      <IconButton
        label={tr('页面操作')}
        className="page-actions-toggle"
        ref={trigger}
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen((value) => !value)}
      >
        <MoreHorizontal />
      </IconButton>
      <div
        id={id}
        className={`page-actions-buttons ${open ? 'is-open' : ''}`}
        role="toolbar"
        aria-label={tr('页面操作')}
      >
        {children}
      </div>
    </div>,
    actions,
  )
}
