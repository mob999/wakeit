import { useLayoutEffect, useRef, type RefObject } from 'react'
import { useTranslation } from 'react-i18next'
import { ExternalLink, FileText, Image } from '../icons'
import { ResultFileLink } from '../files/ResultFileLink'
import type { FileSource } from '../files/links'
import type { ReportLink } from '../moments/reportModel'
import { useReferenceCapacity } from '../moments/useReferenceCapacity'

export function ResultReferences({
  links,
  reading,
  source,
  expanded,
  onExpandedChange,
  scroll,
  onScroll,
  identity,
  preview,
}: {
  links: ReportLink[]
  reading: RefObject<HTMLDivElement | null>
  source?: FileSource
  expanded: boolean
  onExpandedChange: (value: boolean) => void
  scroll: number
  onScroll: (value: number) => void
  identity: string
  preview: boolean
}) {
  const { t } = useTranslation()
  const context = useRef<HTMLElement>(null),
    list = useRef<HTMLDivElement>(null),
    toggle = useRef<HTMLButtonElement>(null)
  const capacity = useReferenceCapacity({
    reading,
    context,
    list,
    toggle,
    count: preview ? links.length : 0,
  })
  useLayoutEffect(() => {
    if (list.current) list.current.scrollTop = scroll
  }, [identity])
  return (
    <aside className="result-references" ref={context} aria-label={t('moments.links')}>
      <h4>{t('moments.links')}</h4>
      <div
        className="result-reference-list"
        ref={list}
        onScroll={(event) => onScroll(event.currentTarget.scrollTop)}
      >
        {links.map((link, index) => (
          <div key={link.href} hidden={preview && !expanded && index >= capacity}>
            {link.local ? (
              <ResultFileLink
                href={link.href}
                source={source}
                accessibleLabel={`${t('moments.links')}: ${link.label}`}
              >
                {link.image ? <Image size={13} /> : <FileText size={13} />}
                <span>{link.label}</span>
              </ResultFileLink>
            ) : (
              <a href={link.href} target="_blank" rel="noopener noreferrer" title={link.href}>
                <ExternalLink size={13} />
                <span>{link.label}</span>
              </a>
            )}
          </div>
        ))}
      </div>
      <button
        type="button"
        className="result-references-more"
        ref={toggle}
        hidden={!preview || (!expanded && links.length <= capacity)}
        aria-expanded={expanded}
        onClick={() => onExpandedChange(!expanded)}
      >
        {expanded ? t('收起') : t('moments.more', { count: links.length - capacity })}
      </button>
    </aside>
  )
}
