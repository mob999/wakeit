import { visit } from 'unist-util-visit'
import type { Root } from 'hast'
export type FileSource = { runId: string; nodeId?: string }
export function isLocalFile(href: string) {
  if (!href || href.startsWith('#') || href.startsWith('//')) return false
  if (/^file:/i.test(href) || /^[a-z]:/i.test(href)) return true
  if (href.split(/[\\/?#]/)[0].includes(':')) return false
  return ![
    '/',
    '/w',
    '/tasks',
    '/runs',
    '/runners',
    '/settings',
    '/login',
    '/authorize',
    '/api',
    '/assets',
  ].some(
    (route) =>
      href === route ||
      (route !== '/' && href.startsWith(`${route}/`)) ||
      href.startsWith(`${route}?`),
  )
}
// Preserve local targets before URL sanitization; authored data attributes never
// grant a target. The server independently parses the persisted report.
export function resultFileLinks() {
  return (tree: Root) =>
    visit(tree, 'element', (node) => {
      delete node.properties.dataWakeitLocal
      const href =
        node.tagName === 'a'
          ? node.properties.href
          : node.tagName === 'img'
            ? node.properties.src
            : null
      if (typeof href === 'string' && isLocalFile(href)) node.properties.dataWakeitLocal = href
    })
}
