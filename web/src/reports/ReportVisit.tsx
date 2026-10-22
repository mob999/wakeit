import { createContext, useContext, useState, useSyncExternalStore, type ReactNode } from 'react'
import { ReportReadingStore } from './readingState'
const Context = createContext<ReportReadingStore | null>(null)
export function ReportVisit({ children }: { children: ReactNode }) {
  const [store] = useState(() => new ReportReadingStore())
  return <Context.Provider value={store}>{children}</Context.Provider>
}
export function useReportReading(key: string) {
  const context = useContext(Context)
  const [fallback] = useState(() => new ReportReadingStore())
  const store = context ?? fallback
  const state = useSyncExternalStore(store.subscribe, () => store.get(key))
  return {
    state,
    update: (patch: Parameters<ReportReadingStore['update']>[1]) => store.update(key, patch),
  }
}
