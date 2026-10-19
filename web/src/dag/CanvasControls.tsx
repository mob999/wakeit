import { type CSSProperties } from 'react'
import { Maximize, Minimize, ZoomIn, ZoomOut } from 'lucide-react'
import { IconButton } from '../ui'
import { t } from '../i18n'
import './controls.css'

export function CanvasControls({
  fullscreen,
  onZoom,
  onToggleFullscreen,
  disabled = false,
  hidden = false,
  style,
}: {
  fullscreen: boolean
  onZoom: (direction: number) => void
  onToggleFullscreen: () => void
  disabled?: boolean
  hidden?: boolean
  style?: CSSProperties
}) {
  return (
    <div
      className="dag-canvas-controls"
      role="toolbar"
      aria-label={t('dag.canvasControls')}
      hidden={hidden}
      style={style}
    >
      <IconButton label={t('dag.zoomOut')} disabled={disabled} onClick={() => onZoom(-1)}>
        <ZoomOut />
      </IconButton>
      <IconButton label={t('dag.zoomIn')} disabled={disabled} onClick={() => onZoom(1)}>
        <ZoomIn />
      </IconButton>
      <IconButton
        label={t(fullscreen ? 'dag.exitFullscreen' : 'dag.fullscreen')}
        aria-pressed={fullscreen}
        disabled={disabled}
        onClick={onToggleFullscreen}
      >
        {fullscreen ? <Minimize /> : <Maximize />}
      </IconButton>
    </div>
  )
}
