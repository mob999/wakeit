import type { Run } from '../api'
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
export type ReportHeading = { label: string; depth: number; index: number }
export type ReportLink = {
  label: string
  href: string
  local: boolean
  index: number
  image?: boolean
}
// Read the sanitized, rendered report, so code samples and rejected URLs never
// become links. Reference previews retain the report's actual execution source.
export function reportIndex(root: ParentNode) {
  const headings: ReportHeading[] = []
  root.querySelectorAll<HTMLElement>('h1,h2,h3,h4,h5,h6').forEach((node, index) => {
    const label = node.textContent?.trim()
    if (label) headings.push({ label, depth: Number(node.tagName[1]), index })
  })
  const links: ReportLink[] = [],
    seen = new Set<string>()
  root
    .querySelectorAll<HTMLElement>('a[href],[data-result-file-href],[data-result-image-href]')
    .forEach((node, index) => {
      const image = node.hasAttribute('data-result-image-href')
      const local = image || node.hasAttribute('data-result-file-href')
      const href =
        node.getAttribute(
          image ? 'data-result-image-href' : local ? 'data-result-file-href' : 'href',
        ) ?? ''
      if (!href || seen.has(href) || (!local && !/^https?:\/\//i.test(href))) return
      seen.add(href)
      links.push({
        href,
        local,
        image,
        label: linkName(
          href,
          node.getAttribute('data-result-image-label') ?? node.textContent ?? '',
        ),
        index,
      })
    })
  return { headings, links }
}
