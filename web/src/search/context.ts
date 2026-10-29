import { createContext, useContext } from 'react'
import type { Scope, Filters } from './filters'

export const SearchContext = createContext<{
  filters: Partial<Record<Scope, Filters>>
  setFilters: (v: Partial<Record<Scope, Filters>>) => void
  open: () => void
  registerGuard: (guard: () => Promise<boolean>) => () => void
  requestLeave: () => Promise<boolean>
}>({
  filters: {},
  setFilters: () => {},
  open: () => {},
  registerGuard: () => () => {},
  requestLeave: async () => true,
})

export const useSearchControls = () => useContext(SearchContext)

export const useFilters = (scope: Scope) => useContext(SearchContext).filters[scope] ?? {}
