import { useState } from 'react'
import { t as tr } from '../i18n'
import { ActionIcon } from '../actions/ActionIcon'
import { ActionMenu, ActionMenuItem, ActionMenuSeparator } from '../actions/ActionMenu'
import { Button, IconButton, Modal } from '../components/ui'
export function PluginPackageActions({
  instance,
  allowInstall,
  disabled,
  upgradeGrants,
  mutate,
}: {
  instance?: { id: string; enabled: boolean }
  allowInstall?: boolean
  disabled: boolean
  upgradeGrants?: string[]
  mutate: (action: string, params: Record<string, unknown>) => Promise<unknown>
}) {
  const [action, setAction] = useState<'install' | 'upgrade' | 'remove' | null>(null)
  const [source, setSource] = useState('')
  return (
    <>
      {!instance && allowInstall && (
        <IconButton
          label={tr('plugins.install')}
          disabled={disabled}
          onClick={() => setAction('install')}
        >
          <ActionIcon name="add" />
        </IconButton>
      )}
      {instance && (
        <ActionMenu label={tr('plugins.manage')}>
          {allowInstall && (
            <ActionMenuItem icon="add" disabled={disabled} onSelect={() => setAction('install')}>
              {tr('plugins.install')}
            </ActionMenuItem>
          )}
          <ActionMenuItem icon="refresh" disabled={disabled} onSelect={() => setAction('upgrade')}>
            {tr('plugins.upgrade')}
          </ActionMenuItem>
          <ActionMenuSeparator />
          <ActionMenuItem
            icon="archive"
            danger
            disabled={disabled || instance.enabled}
            reason={instance.enabled ? tr('plugins.disableBeforeRemove') : undefined}
            onSelect={() => setAction('remove')}
          >
            {tr('plugins.remove')}
          </ActionMenuItem>
        </ActionMenu>
      )}
      <Modal
        open={action !== null}
        onOpenChange={(v) => !v && setAction(null)}
        title={tr('plugins.' + (action ?? 'install'))}
        actions={
          <>
            <Button onClick={() => setAction(null)}>{tr('取消')}</Button>
            <Button
              variant="solid"
              danger={action === 'remove'}
              disabled={disabled || (action === 'install' && !source)}
              onClick={async () => {
                if (action === 'upgrade' && instance && upgradeGrants) {
                  if (
                    !(await mutate('authorize', {
                      instance_id: instance.id,
                      grants: upgradeGrants,
                    }))
                  )
                    return
                }
                const result = await mutate(
                  action!,
                  action === 'install' ? { source } : { instance_id: instance!.id },
                )
                if (result) setAction(null)
              }}
            >
              {tr('plugins.' + (action ?? 'install'))}
            </Button>
          </>
        }
      >
        <div className="p-5">
          {action === 'install' ? (
            <label>
              {tr('plugins.source')}
              <input
                value={source}
                onChange={(e) => setSource(e.target.value)}
                placeholder="https://github.com/owner/plugin"
              />
            </label>
          ) : (
            <>
              <p>
                {tr(
                  action === 'upgrade' ? 'plugins.upgradeDescription' : 'plugins.removeDescription',
                )}
              </p>
              {action === 'upgrade' && upgradeGrants && (
                <div className="plugin-permissions">
                  {upgradeGrants.map((grant) => (
                    <div key={grant}>{grant}</div>
                  ))}
                </div>
              )}
            </>
          )}
        </div>
      </Modal>
    </>
  )
}
