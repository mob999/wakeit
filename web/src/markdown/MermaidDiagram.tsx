import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useEffect, useState } from 'react'
import DOMPurify from 'dompurify'

let serial: Promise<unknown> = Promise.resolve()
let sequence = 0
const cache = new Map<string, { svg: string; width: number; height: number }>()

async function render(source: string, dark: boolean) {
  const key = `${dark}\0${source}`
  const cached = cache.get(key)
  if (cached) return cached
  const { default: mermaid } = await import('mermaid')
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: 'strict',
    suppressErrorRendering: true,
    theme: dark ? 'dark' : 'neutral',
    fontFamily: 'system-ui, sans-serif',
    htmlLabels: false,
    flowchart: { htmlLabels: false },
    maxTextSize: 50000,
    maxEdges: 500,
    secure: [
      'securityLevel',
      'startOnLoad',
      'suppressErrorRendering',
      'maxTextSize',
      'maxEdges',
      'htmlLabels',
      'flowchart',
      'fontFamily',
      'altFontFamily',
      'themeCSS',
      'themeVariables',
      'theme',
    ],
  })
  const container = document.createElement('div')
  container.style.cssText =
    'position:fixed;left:-100000px;top:0;visibility:hidden;pointer-events:none'
  document.body.appendChild(container)
  try {
    const { svg } = await mermaid.render(`wakeit-diagram-${++sequence}`, source, container)
    const clean = DOMPurify.sanitize(svg, {
      USE_PROFILES: { svg: true, svgFilters: true },
      FORBID_TAGS: ['script', 'foreignObject', 'image', 'a'],
      FORBID_ATTR: ['href', 'xlink:href'],
    })
    const parsed = new DOMParser().parseFromString(clean, 'image/svg+xml')
    const box = parsed.documentElement
      .getAttribute('viewBox')
      ?.split(/[\s,]+/)
      .map(Number)
    const size =
      box?.length === 4 && box.every(Number.isFinite) && box[2] > 0 && box[3] > 0
        ? { width: box[2], height: box[3] }
        : { width: 640, height: 320 }
    const value = { svg: clean, ...size }
    if (cache.size >= 16) cache.delete(cache.keys().next().value!)
    if (clean.length + source.length < 250000) cache.set(key, value)
    return value
  } finally {
    container.remove()
  }
}

export default function MermaidDiagram({ source }: { source: string }) {
  useTranslation()
  const [dark, setDark] = useState(() => document.documentElement.dataset.theme === 'dark')
  const [diagram, setDiagram] = useState<Awaited<ReturnType<typeof render>> | null>(null)
  const [error, setError] = useState(false)
  useEffect(() => {
    const observer = new MutationObserver(() =>
      setDark(document.documentElement.dataset.theme === 'dark'),
    )
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    })
    return () => observer.disconnect()
  }, [])
  useEffect(() => {
    let active = true
    setDiagram(null)
    setError(false)
    const job = serial.then(async () => (active ? render(source, dark) : null))
    serial = job.catch(() => undefined)
    job
      .then((value) => {
        if (active) setDiagram(value)
      })
      .catch(() => {
        if (active) setError(true)
      })
    return () => {
      active = false
    }
  }, [source, dark])
  const color = dark ? '#fafafa' : '#0a0a0a'
  const background = dark ? '#0a0a0a' : '#fff'
  const documentHtml = diagram
    ? `<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; font-src 'none'; img-src 'none'; base-uri 'none'; form-action 'none'"><style>html,body{margin:0;background:${background};color:${color};font-family:system-ui,sans-serif}svg{display:block;width:100%;max-width:100%;height:auto;margin:auto}</style></head><body>${diagram.svg}</body></html>`
    : ''
  return (
    <figure
      className="mermaid-diagram"
      aria-label={tr('Mermaid 图表')}
      aria-busy={!diagram && !error}
    >
      {diagram && (
        <div className="mermaid-graphic">
          <iframe
            title={tr('Mermaid 图表')}
            sandbox=""
            referrerPolicy="no-referrer"
            srcDoc={documentHtml}
            style={{
              aspectRatio: `${diagram.width} / ${diagram.height}`,
              minHeight: 100,
              maxHeight: 640,
            }}
          />
        </div>
      )}
      {error && (
        <p className="mermaid-error" role="status">
          {tr('图表无法渲染')}
        </p>
      )}
      <details open={error || (!diagram && !error) || undefined}>
        <summary>{tr('图表源码')}</summary>
        <pre>
          <code>{source}</code>
        </pre>
      </details>
    </figure>
  )
}
