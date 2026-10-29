import { useEffect, useRef, useState, type CSSProperties } from 'react'

export function PaneResizeHandle({
  label,
  width,
  min,
  max,
  onResize,
  onReset,
  style,
  edge = 'left',
  className = '',
}: {
  label: string
  width: number
  min: number
  max: number
  onResize: (width: number) => void
  onReset: () => void
  style: CSSProperties
  edge?: 'left' | 'right'
  className?: string
}) {
  const [resizing, setResizing] = useState(false)
  const drag = useRef<{ pointer: number; element: HTMLDivElement } | null>(null)
  const resize = useRef(onResize)
  resize.current = onResize
  useEffect(() => {
    if (!resizing) return
    const move = (event: PointerEvent) => {
      const active = drag.current
      if (!active || event.pointerId !== active.pointer) return
      const pane = active.element.parentElement!.getBoundingClientRect()
      const next = edge === 'left' ? event.clientX - pane.left : pane.right - event.clientX
      resize.current(Math.max(min, Math.min(max, next)))
    }
    const end = (event: PointerEvent) => {
      const active = drag.current
      if (!active || event.pointerId !== active.pointer) return
      drag.current = null
      if (active.element.hasPointerCapture(active.pointer))
        active.element.releasePointerCapture(active.pointer)
      setResizing(false)
    }
    const blur = () => {
      drag.current = null
      setResizing(false)
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', end)
    window.addEventListener('pointercancel', end)
    window.addEventListener('blur', blur)
    return () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', end)
      window.removeEventListener('pointercancel', end)
      window.removeEventListener('blur', blur)
    }
  }, [resizing, edge, min, max])
  return (
    <div
      className={`resize-handle ${className}`.trim()}
      style={style}
      data-resizing={resizing || undefined}
      role="separator"
      aria-label={label}
      aria-orientation="vertical"
      aria-valuenow={width}
      aria-valuemin={min}
      aria-valuemax={max}
      tabIndex={0}
      onPointerDown={(e) => {
        if (e.button !== 0) return
        e.preventDefault()
        e.currentTarget.focus()
        drag.current = { pointer: e.pointerId, element: e.currentTarget }
        e.currentTarget.setPointerCapture(e.pointerId)
        setResizing(true)
      }}
      onDoubleClick={onReset}
      onKeyDown={(e) => {
        const step = edge === 'left' ? 10 : -10
        const next = { ArrowLeft: width - step, ArrowRight: width + step, Home: min, End: max }[
          e.key
        ]
        if (next === undefined) return
        onResize(Math.max(min, Math.min(max, next)))
        e.preventDefault()
      }}
    />
  )
}
