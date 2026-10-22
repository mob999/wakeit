import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { CircleAlert, Flag, Monitor } from 'lucide-react'
import { AgentLogo } from '../AgentLogo'
import { TooltipTarget } from '../ui'
import { t } from '../i18n'
import './node-card.css'

export function OutputBadge() {
  useTranslation()
  return (
    <span className="dag-output-badge" aria-label={t('dag.outputNode')} title={t('dag.outputNode')}>
      <Flag size={14} aria-hidden="true" />
      <span>{t('dag.output')}</span>
    </span>
  )
}

export function NodeCardContent({
  name,
  runner,
  agent,
  logo,
  output,
  invalid = false,
  actions,
  status,
}: {
  name: string
  runner: string
  agent: string
  logo?: string | null
  output: boolean
  invalid?: boolean
  actions?: ReactNode
  status?: ReactNode
}) {
  useTranslation()
  return (
    <>
      <span className="dag-node-card-header">
        <TooltipTarget label={runner}>
          <span className="dag-node-card-runner">
            <Monitor size={12} aria-hidden="true" />
            <span>{runner}</span>
          </span>
        </TooltipTarget>
        <TooltipTarget label={agent}>
          <span className="dag-node-card-agent">
            <AgentLogo name={agent} logo={logo} />
            <span>{agent}</span>
          </span>
        </TooltipTarget>
        {actions && (
          <span className="dag-node-card-actions nodrag nopan" onClick={(e) => e.stopPropagation()}>
            {actions}
          </span>
        )}
      </span>
      <span className="dag-node-card-name">
        <strong title={name}>{name}</strong>
      </span>
      <span className="dag-node-card-footer">
        {output && <OutputBadge />}
        {invalid && (
          <span
            className="dag-node-card-invalid"
            aria-label={t('dag.checkNode')}
            title={t('dag.checkNode')}
          >
            <CircleAlert size={13} aria-hidden="true" />
          </span>
        )}
        {status && <span className="dag-node-card-status">{status}</span>}
      </span>
    </>
  )
}
