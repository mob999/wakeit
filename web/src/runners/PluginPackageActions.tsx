import { useState } from 'react'
import { t as tr } from '../i18n'
import { ActionIcon } from '../actions/ActionIcon'
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
      {allowInstall && (
        <IconButton
          label={tr('plugins.install')}
          disabled={disabled}
          onClick={() => setAction('install')}
        >
          <ActionIcon name="add" />
        </IconButton>
      )}
      {instance && (
        <>
          <IconButton
            label={tr('plugins.upgrade')}
            disabled={disabled}
            onClick={() => setAction('upgrade')}
          >
            <ActionIcon name="refresh" />
          </IconButton>
          <IconButton
            label={tr('plugins.remove')}
            disabled={disabled || instance.enabled}
            onClick={() => setAction('remove')}
          >
            <ActionIcon name="archive" />
          </IconButton>
        </>
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
              {tr('确认')}
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
