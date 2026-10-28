import { useRef, useState } from 'react'
import * as Menu from '@radix-ui/react-dropdown-menu'
import { ChevronDown } from '../icons'
import { t } from '../i18n'
import { stateNames, type Run, type DagRunNode } from '../api'
import { Status } from '../Status'
import { RunReason } from '../runs/RunReason'
import { shortId } from './identity'
import { nodeIssues } from './nodeIssues'
import './DagRunStatus.css'

export function DagRunStatus({
  run,
  nodes,
  onSelect,
}: {
  run: Run
  nodes: DagRunNode[]
  onSelect: (id: string) => void
}) {
  const { issues, skipped } = nodeIssues(nodes, run.state)
  const [open, setOpen] = useState(false)
  const status = useRef<HTMLDivElement>(null)
  const navigating = useRef(false)
  const names = new Map<string, number>()
  for (const node of nodes) names.set(node.name, (names.get(node.name) ?? 0) + 1)
  const row = (node: DagRunNode) => (
    <Menu.Item
      key={node.id}
      textValue={`${node.name} ${node.id}`}
      aria-label={`${node.name} ${stateNames[node.state] ?? node.state} ${node.id}`}
      onSelect={() => {
        navigating.current = true
        onSelect(node.id)
      }}
    >
      <span className="dag-issue-identity" title={`${node.name} · ${node.id}`}>
        <span>{node.name}</span>
        {(names.get(node.name) ?? 0) > 1 && <small className="mono">{shortId(node.id)}</small>}
      </span>
      <Status state={node.state} />
    </Menu.Item>
  )
  return (
    <div
      className="dag-run-meta"
      role="group"
      tabIndex={-1}
      ref={status}
      aria-label={t('reading.runStatus')}
    >
      <RunReason
        state={run.state}
        reason={run.reason}
        accessibleLabel={t('reading.runStatusReason', {
          state: stateNames[run.state] ?? run.state,
          reason: run.reason,
        })}
      >
        <Status state={run.state} />
      </RunReason>
      {(issues.length > 0 || open) && (
        <Menu.Root open={open} onOpenChange={setOpen}>
          <Menu.Trigger asChild>
            <button type="button" className="dag-node-issues-trigger">
              {t('dag.nodeIssues', { count: issues.length })}
              <ChevronDown size={14} aria-hidden="true" />
            </button>
          </Menu.Trigger>
          <Menu.Portal>
            <Menu.Content
              className="dropdown dag-node-issues-menu"
              align="end"
              sideOffset={4}
              collisionPadding={12}
              onCloseAutoFocus={(event) => {
                if (navigating.current) {
                  event.preventDefault()
                  navigating.current = false
                } else if (!issues.length) {
                  event.preventDefault()
                  status.current?.focus({ preventScroll: true })
                }
              }}
            >
              <div className="dag-issue-head" aria-hidden="true">
                <span>{t('dag.nodes')}</span>
                <span>{t('状态')}</span>
              </div>
              {issues.map(row)}
              {!issues.length && (
                <p className="dag-issues-empty quiet text-muted">{t('dag.noNodeIssues')}</p>
              )}
              {issues.length > 0 && skipped.length > 0 && (
                <Menu.Group>
                  <Menu.Label className="dag-issue-group-label">{t('已跳过')}</Menu.Label>
                  {skipped.map(row)}
                </Menu.Group>
              )}
            </Menu.Content>
          </Menu.Portal>
        </Menu.Root>
      )}
    </div>
  )
}
