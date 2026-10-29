import { createContext, useContext, useEffect, type ReactNode } from 'react'

export const ToastContext = createContext<(content: ReactNode) => void>(() => {})
export const useToast = () => useContext(ToastContext)

export function ErrorToast({
  error,
  actions,
  id,
}: {
  error?: Error | string | null
  actions?: ReactNode
  id?: string
}) {
  const toast = useToast()
  const message = error instanceof Error ? error.message : error
  useEffect(() => {
    if (message) toast(message)
  }, [message, toast])
  return (
    <>
      {id && (
        <span id={id} className="sr-only">
          {message}
        </span>
      )}
      {actions && <div className="request-retry">{actions}</div>}
    </>
  )
}
