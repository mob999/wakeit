export type ReadingAnchor = { heading: string | null; offset: number }
export type ReportReadingState = {
  outlineExpanded: boolean
  referencesExpanded: boolean
  outlineScroll: number
  referencesScroll: number
  activeHeading: string | null
  anchor: ReadingAnchor | null
}
export class ReportReadingStore {
  private states = new Map<string, ReportReadingState>()
  private listeners = new Set<() => void>()
  get = (key: string): ReportReadingState => {
    let value = this.states.get(key)
    if (!value) {
      value = {
        outlineExpanded: false,
        referencesExpanded: false,
        outlineScroll: 0,
        referencesScroll: 0,
        activeHeading: null,
        anchor: null,
      }
      this.states.set(key, value)
    }
    return value
  }
  update = (key: string, patch: Partial<ReportReadingState>) => {
    const value = this.get(key)
    if (
      Object.entries(patch).every(
        ([name, next]) => value[name as keyof ReportReadingState] === next,
      )
    )
      return
    this.states.set(key, { ...value, ...patch })
    this.listeners.forEach((listener) => listener())
  }
  subscribe = (listener: () => void) => {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }
}
