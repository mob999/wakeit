import type { MomentEntry } from '../api/types'

// The list uses the bounded API excerpt, never an additional model request or full report fetch.
export function answerSummary(entry: Pick<MomentEntry, 'format' | 'preview' | 'outputs'>): {
  title: string
  continuation: string
} {
  const excerpts =
    entry.format === 'dag'
      ? (entry.outputs ?? [])
          .filter((output) => output.format === 'markdown')
          .map((output) => output.preview)
      : entry.format === 'markdown'
        ? [entry.preview]
        : []
  for (const excerpt of excerpts) {
    const lines = (excerpt ?? '').replace(/```[^]*?(?:```|$)|~~~[^]*?(?:~~~|$)/g, '').split('\n')
    const textLines: string[] = []
    for (const line of lines) {
      if (/^\s*(?:\[[^\]]+\]:|\|?\s*:?-{3,}|<\/?(?:script|style)\b)/i.test(line)) continue
      const text = line
        .replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
        .replace(/<[^>]*>/g, '')
        .replace(/https?:\/\/\S+/g, '')
        .replace(/^\s*(?:#{1,6}\s+|>\s*|[-*+]\s+|\d+[.)]\s+)/, '')
        .replace(/[*_`~]/g, '')
        .replace(/\s+/g, ' ')
        .trim()
      if (text && !/^(?:摘要|总结|结论|结果|summary|conclusion|results?)[:：]?$/i.test(text))
        textLines.push(text)
    }
    if (textLines.length) return { title: textLines[0], continuation: textLines.slice(1).join(' ') }
  }
  return { title: '', continuation: '' }
}
