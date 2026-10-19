import { createContext, useContext, useState, type Dispatch, type SetStateAction } from 'react'
export type EditorMemory = Record<string, unknown>
export const EditorMemoryContext = createContext<{
  active?: boolean
  value: EditorMemory
  update: Dispatch<SetStateAction<EditorMemory>>
} | null>(null)
export function useEditorValue<T>(key: string, initial: T): [T, Dispatch<SetStateAction<T>>] {
  const memory = useContext(EditorMemoryContext)
  const [local, setLocal] = useState(initial)
  const value = memory && key in memory.value ? (memory.value[key] as T) : local
  const update: Dispatch<SetStateAction<T>> = (next) => {
    if (memory)
      memory.update((previous) => ({
        ...previous,
        [key]:
          typeof next === 'function'
            ? (next as (value: T) => T)((key in previous ? previous[key] : initial) as T)
            : next,
      }))
    else setLocal(next)
  }
  return [value, update]
}
