import {
  createContext,
  useContext,
  useEffect,
  isValidElement,
  Children,
  type ReactNode,
} from 'react'
import { Button, IconButton } from './ui'

export const ToastContext = createContext<(content: ReactNode) => void>(() => {})
export const useToast = () => useContext(ToastContext)
function text(content: ReactNode): string {
  return Children.toArray(content)
    .map((child) =>
      isValidElement<{ children?: ReactNode }>(child)
        ? child.type === Button || child.type === IconButton || child.type === 'button'
          ? ''
          : text(child.props.children)
        : typeof child === 'string' || typeof child === 'number'
          ? String(child)
          : '',
    )
    .join(' ')
    .replace(/\s+/g, ' ')
    .trim()
}
function actions(content: ReactNode): ReactNode[] {
  return Children.toArray(content).flatMap((child) =>
    !isValidElement<{ children?: ReactNode }>(child)
      ? []
      : child.type === Button || child.type === IconButton || child.type === 'button'
        ? [child]
        : actions(child.props.children),
  )
}
export function ErrorToast({
  children,
  error,
  id,
}: {
  children?: ReactNode
  error?: Error | string | null
  id?: string
}) {
  const toast = useToast()
  const content = children ?? (error instanceof Error ? error.message : error)
  const key = text(content)
  useEffect(() => {
    if (key) toast(key)
  }, [key, toast])
  const controls = actions(children)
  return (
    <>
      {id && (
        <span id={id} className="sr-only">
          {key}
        </span>
      )}
      {controls.length > 0 && <div className="request-retry">{controls}</div>}
    </>
  )
}
