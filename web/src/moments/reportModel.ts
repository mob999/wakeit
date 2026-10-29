import type { Run } from '../api/types'
export function duration(start?: string | null, end?: string | null) {
  if (!start || !end) return null
  const ms = Date.parse(end) - Date.parse(start)
  if (!Number.isFinite(ms) || ms < 0) return null
  const seconds = Math.floor(ms / 1000)
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  return minutes < 60
    ? `${minutes}m ${seconds % 60}s`
    : `${Math.floor(minutes / 60)}h ${minutes % 60}m`
}
export function retriedNodes(run?: Run) {
  return run?.nodes?.filter((node) => (node.attempt_count ?? 0) > 1).length ?? 0
}
export function linkName(href: string, label: string) {
  const short = label.trim()
  if (short && short !== href) return short
  try {
    const path = href.split(/[?#]/)[0].replace(/\/$/, '')
    return decodeURIComponent(path.split(/[\\/]/).at(-1) || href)
  } catch {
    return href
  }
}
export type ReportHeading = { label: string; depth: number; index: number; key: string }
export type ReportLink = {
  label: string
  href: string
  local: boolean
  index: number
  image?: boolean
}
export function referenceIdentity(href: string, local: boolean, cwd?: string) {
  if (!local) return `url:${href}`
  const original = `raw:${href}`
  if (/[\u0000-\u001f\u007f]/.test(href)) return original
  let value = href
  if (/^file:/i.test(value)) {
    if (!/^file:\/\/\/(?!\/)/i.test(value)) return original
    value = value.slice(7)
  }
  value = value.split(/[?#]/)[0]
  try {
    value = decodeURIComponent(value)
  } catch {
    return original
  }
  if (/[\u0000-\u001f\u007f]/.test(value) || /^(\/\/|\\\\)/.test(value)) return original
  const windows = !!cwd && /^[a-z]:[\\/]/i.test(cwd)
  if (windows) {
    value = value.replace(/^\/([a-z]:[\\/])/i, '$1').replace(/\\/g, '/')
    if (!/^[a-z]:\//i.test(value) && !value.startsWith('/')) value = `${cwd}/${value}`
    value = value
      .replace(/\\/g, '/')
      .replace(/^([a-z]):/i, (_, drive: string) => `${drive.toUpperCase()}:`)
  } else {
    if (value.includes('\\') || /^[a-z]:/i.test(value)) return original
    if (!value.startsWith('/') && cwd?.startsWith('/')) value = `${cwd}/${value}`
  }
  // Dot and empty components are lexical aliases. Keep '..', case and Unicode:
  // resolving them requires the Runner filesystem, including symlink semantics.
  const absolute = value.startsWith('/')
  const normalized = value
    .split('/')
    .filter((part) => part && part !== '.')
    .join('/')
  return `${windows ? 'windows' : 'posix'}:${absolute ? '/' : ''}${normalized}`
}
// Read the sanitized, rendered report, so code samples and rejected URLs never
// become links. Reference previews retain the report's actual execution source.
export function reportIndex(root: ParentNode, cwd?: string) {
  const headings: ReportHeading[] = []
  const occurrences = new Map<string, number>()
  root.querySelectorAll<HTMLElement>('h1,h2,h3,h4,h5,h6').forEach((node, index) => {
    const label = node.textContent?.trim()
    if (label) {
      const depth = Number(node.tagName[1]),
        identity = `${depth}:${label}`
      const occurrence = occurrences.get(identity) ?? 0
      occurrences.set(identity, occurrence + 1)
      headings.push({ label, depth, index, key: `${identity}:${occurrence}` })
    }
  })
  const links: ReportLink[] = [],
    seen = new Map<string, { item: ReportLink; quality: number }>()
  root
    .querySelectorAll<HTMLElement>('a[href],[data-result-file-href],[data-result-image-href]')
    .forEach((node, index) => {
      const image = node.hasAttribute('data-result-image-href')
      const local = image || node.hasAttribute('data-result-file-href')
      const href =
        node.getAttribute(
          image ? 'data-result-image-href' : local ? 'data-result-file-href' : 'href',
        ) ?? ''
      if (!href || (!local && !/^https?:\/\//i.test(href))) return
      const nestedImage = node.querySelector<HTMLElement>('[data-result-image-label]')
      const alt =
        node.getAttribute('data-result-image-label') ??
        nestedImage?.getAttribute('data-result-image-label')
      const label = (alt ?? node.textContent ?? '').trim()
      const explicit = !!label && label !== href
      const quality = explicit ? (image ? 3 : 2) : 0
      const identity = referenceIdentity(href, local, cwd)
      const existing = seen.get(identity)
      if (existing) {
        existing.item.image ||= image
        if (quality > existing.quality) {
          existing.item.label = linkName(href, label)
          existing.quality = quality
        }
        return
      }
      const item: ReportLink = {
        href,
        local,
        image,
        label: linkName(href, label),
        index,
      }
      seen.set(identity, { item, quality })
      links.push(item)
    })
  return { headings, links }
}
