import { createContext } from 'react'
// Keep the report owner of an open file preview mounted during windowing.
export const PreviewActivity = createContext<(id: string, open: boolean) => void>(() => {})
