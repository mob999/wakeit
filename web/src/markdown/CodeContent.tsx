import { useMemo } from 'react'
import { MarkdownContent } from './MarkdownContent'

export function isJsonCode(value: unknown) {
  if (value == null) return false
  if (typeof value !== 'string') return true
  try {
    JSON.parse(value)
    return true
  } catch {
    return false
  }
}

export function CodeContent({
  text,
  json = false,
  className = '',
}: {
  text: string
  json?: boolean
  className?: string
}) {
  const markdown = useMemo(() => {
    if (!json) return ''
    const longest = (text.match(/`+/g) ?? []).reduce(
      (length, match) => Math.max(length, match.length),
      2,
    )
    const fence = '`'.repeat(longest + 1)
    return `${fence}json\n${text}\n${fence}`
  }, [text, json])
  return json ? (
    <div className={`markdown-result code-content ${className}`}>
      <MarkdownContent text={markdown} />
    </div>
  ) : (
    <pre className={`code ${className}`}>{text}</pre>
  )
}
