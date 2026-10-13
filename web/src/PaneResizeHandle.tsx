import { useState, type CSSProperties } from 'react'

export function PaneResizeHandle({
  label,
  width,
  min,
  max,
  onResize,
  onReset,
  style,
}: {
  label: string
  width: number
  min: number
  max: number
  onResize: (width: number) => void
  onReset: () => void
  style: CSSProperties
}) {
  const [resizing, setResizing] = useState(false)
  return (
    <div
      className="resize-handle"
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
        e.currentTarget.setPointerCapture(e.pointerId)
        setResizing(true)
      }}
      onPointerMove={(e) => {
        if (e.currentTarget.hasPointerCapture(e.pointerId)) {
          const pane = e.currentTarget.parentElement!.getBoundingClientRect()
          onResize(Math.max(min, Math.min(max, e.clientX - pane.left)))
        }
      }}
      onPointerUp={(e) => {
        if (e.currentTarget.hasPointerCapture(e.pointerId))
          e.currentTarget.releasePointerCapture(e.pointerId)
        setResizing(false)
      }}
      onLostPointerCapture={() => setResizing(false)}
      onDoubleClick={onReset}
      onKeyDown={(e) => {
        const next = { ArrowLeft: width - 10, ArrowRight: width + 10, Home: min, End: max }[e.key]
        if (next === undefined) return
        onResize(Math.max(min, Math.min(max, next)))
        e.preventDefault()
      }}
    />
  )
}
