export function valueText(value: unknown): string {
  return value == null ? '' : typeof value === 'string' ? value : JSON.stringify(value, null, 2)
}
export function visibleSummary(value: unknown): string {
  return (
    valueText(value)
      .replace(/<[^>]*>/g, ' ')
      .replace(/!?(?:\[([^\]]*)\])\([^)]*\)/g, '$1')
      .replace(/^[ \t#>*`~-]+/gm, '')
      .replace(/[*_`]/g, '')
      .split(/\n\s*\n/)
      .find((v) => v.trim())
      ?.replace(/\s+/g, ' ')
      .trim()
      .slice(0, 240) ?? ''
  )
}
export function inputSummary(value: unknown): string {
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    const input = value as Record<string, unknown>
    for (const key of ['command', 'cmd', 'file_path', 'path', 'file', 'url', 'query', 'pattern'])
      if (typeof input[key] === 'string' && input[key])
        return input[key].replace(/\s+/g, ' ').trim().slice(0, 240)
  }
  return valueText(value).replace(/\s+/g, ' ').trim().slice(0, 240)
}
export function previewLines(text: string, full: boolean) {
  const lines = text.split('\n')
  return { text: full ? text : lines.slice(0, 12).join('\n'), truncated: lines.length > 12 }
}
export type RunTab = 'output' | 'trace' | 'details'
export type ReadingState = {
  viewTab?: RunTab
  expanded: Record<string, boolean>
  payload: Record<string, boolean>
  full: Record<string, boolean>
  autoExpand: boolean
  autoId?: string
  follow: boolean
  traceVisited: boolean
  unseen: string[]
  jump: number
  anchor?: { id: string; offset: number }
  highlighted: boolean
  resultReady: boolean
}
export function runTab(state: ReadingState, outputAvailable: boolean): RunTab {
  return (
    state.viewTab ?? (outputAvailable && (state.follow || !state.traceVisited) ? 'output' : 'trace')
  )
}
export function freshReading(): ReadingState {
  return {
    expanded: {},
    payload: {},
    full: {},
    autoExpand: false,
    follow: false,
    traceVisited: false,
    unseen: [],
    jump: 0,
    highlighted: false,
    resultReady: false,
  }
}
export function isExpanded(state: ReadingState, id: string, latest: string | undefined) {
  return state.expanded[id] ?? (state.autoExpand && id === (state.autoId ?? latest))
}
export function followingTail(distance: number) {
  return distance <= 48
}
