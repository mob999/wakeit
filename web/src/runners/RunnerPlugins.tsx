import { useEffect, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { api, apiUrl, workspaceBase } from '../api/client'
import type { Runner } from '../api/types'
import { ActionIcon } from '../actions/ActionIcon'
import { ActionMenu, ActionMenuItem } from '../actions/ActionMenu'
import { Button, IconButton, Modal } from '../components/ui'
import { ChoicePicker } from '../components/ChoicePicker'
import { DetailTabs } from '../components/DetailTabs'
import { ErrorToast } from '../components/Toast'
import { PluginFields, type Schema } from './PluginFields'
import { PluginPackageActions } from './PluginPackageActions'
import { PluginCreateDialog } from './PluginCreateDialog'
import {
  PluginSetupError,
  submitPluginSetup,
  type PluginSetupSession,
  type PluginSetupDraft,
} from './pluginSetup'
import './plugins.css'
type Declaration = {
  name: string
  description: string
  translations?: Record<string, Record<string, string>>
  config: Schema
  permissions: string[]
  credentials: Record<string, { label: string; description: string; required: boolean }>
  handlers: Record<string, { events: string[]; description: string }>
  hooks: Record<string, { stage: string; description: string }>
  agents?: Record<string, { name: string }>
  actions: Record<
    string,
    { label: string; description: string; input: Schema; side_effects: boolean }
  >
}
type Instance = {
  id: string
  source: string
  extension: string
  name: string
  version: string
  state: string
  enabled: boolean
  revision: number
  applied_revision?: number
  is_default?: boolean
  config?: unknown
  grants: string[]
  credential_status: Record<string, boolean>
  declaration: Declaration
}
type Catalog = {
  instances: Instance[]
  packages: {
    source: string
    name: string
    version: string
    extensions: Record<string, Declaration>
  }[]
  remote_management?: boolean
  updated_at?: string
}
type Binding = {
  id: string
  instance_id: string
  runner_id: string
  kind: 'event' | 'hook'
  event: string
  handler: string
  enabled: boolean
  filter: Record<string, unknown>
  position: number
  failure_policy: string
  timeout_seconds: number
  max_attempts: number
  expires_seconds: number
  revision: number
}
export function RunnerPlugins({
  runner,
  admin,
  canAct,
  toast,
}: {
  runner: Runner
  admin: boolean
  canAct: boolean
  toast: (message: string) => void
}) {
  const { i18n } = useTranslation()
  const client = useQueryClient()
  const [selected, setSelected] = useState('')
  const [tab, setTab] = useState('config')
  const [creating, setCreating] = useState(false)
  const setupSession = useRef<PluginSetupSession>({ id: crypto.randomUUID() })
  const [setupSaved, setSetupSaved] = useState(false)
  const [setupLocked, setSetupLocked] = useState(false)
  const [busy, setBusy] = useState(false)
  const control = (action: string, params: Record<string, unknown> = {}) =>
    api(`/runners/${runner.id}/plugins/control`, 'POST', { action, params })
  const catalog = useQuery<Catalog>({
    queryKey: ['plugins', runner.id, runner.online],
    queryFn: () => (runner.online ? control('catalog') : api(`/runners/${runner.id}/plugins`)),
    refetchInterval: 10000,
  })
  const rawInstance =
    catalog.data?.instances.find((i) => i.id === selected) ?? catalog.data?.instances[0]
  const instance = rawInstance
    ? {
        ...rawInstance,
        declaration: localized(rawInstance.declaration, i18n.resolvedLanguage ?? i18n.language),
      }
    : undefined
  const refresh = () => client.invalidateQueries({ queryKey: ['plugins', runner.id] })
  const mutate = async (action: string, params: Record<string, unknown>) => {
    setBusy(true)
    try {
      const result = await control(action, params)
      await refresh()
      if (action === 'configure' || action === 'authorize') toast(tr('已保存'))
      return result
    } catch (e) {
      toast((e as Error).message)
    } finally {
      setBusy(false)
    }
  }
  const resources =
    catalog.data?.packages.flatMap((p) =>
      Object.entries(p.extensions ?? {}).map(([id, raw]) => {
        const d = localized(raw, i18n.resolvedLanguage ?? i18n.language)
        return {
          value: JSON.stringify([p.source, id]),
          label: d.name,
          packageName: p.name,
          version: p.version,
          description: d.description,
          agents: Object.values(d.agents ?? {}).map((agent) => agent.name),
          events: Object.values(d.handlers).flatMap((handler) => handler.events),
          hooks: Object.values(d.hooks).map((hook) => hook.stage),
          actions: Object.values(d.actions).map((action) => action.label),
          config: d.config,
          credentials: d.credentials,
          permissions: d.permissions,
        }
      }),
    ) ?? []
  return (
    <div className="plugin-workbench">
      <aside className="plugin-list">
        <div className="pane-heading">
          <h2>{tr('plugins.title')}</h2>
          {admin && (
            <IconButton
              label={tr('pluginCreate.title')}
              disabled={!runner.online}
              onClick={() => {
                setupSession.current = { id: crypto.randomUUID() }
                setSetupSaved(false)
                setSetupLocked(false)
                setCreating(true)
              }}
            >
              <ActionIcon name="add" />
            </IconButton>
          )}
        </div>
        {admin && catalog.data?.remote_management && !instance && (
          <PluginPackageActions allowInstall disabled={!runner.online || busy} mutate={mutate} />
        )}
        {catalog.error && <ErrorToast error={catalog.error.message} />}
        {catalog.isPending && <p className="p-4 text-muted">{tr('正在加载…')}</p>}
        {catalog.data?.instances.map((i) => (
          <button
            key={i.id}
            className={`plugin-list-row ${instance?.id === i.id ? 'is-selected' : ''}`}
            onClick={() => setSelected(i.id)}
          >
            <span>{i.name}</span>
            <span
              className={`status ${i.state === 'running' ? 'succeeded' : i.enabled ? 'warning' : ''}`}
            >
              <i />
            </span>
          </button>
        ))}
        {catalog.data && !catalog.data.instances.length && (
          <p className="p-4 text-muted">{tr('plugins.empty')}</p>
        )}
      </aside>
      <div className="plugin-detail">
        {instance ? (
          <>
            <div className="plugin-title">
              <div>
                <h2>{instance.name}</h2>
                <span className="text-muted">
                  {instance.version}
                  {instance.is_default && Object.keys(instance.declaration.agents ?? {}).length > 0
                    ? ` · ${tr('plugins.defaultInstance')}`
                    : ''}
                </span>
              </div>
              <div className="plugin-title-actions">
                <span className="text-muted">
                  {!runner.online ? tr('设备离线') : tr(`plugins.state.${instance.state}`)}
                </span>
                {admin && (
                  <IconButton
                    label={tr(instance.enabled ? 'plugins.disable' : 'plugins.enable', {
                      name: instance.name,
                    })}
                    disabled={!runner.online || busy}
                    disabledReason={tr(!runner.online ? '设备离线' : 'plugins.busy')}
                    onClick={() =>
                      mutate(instance.enabled ? 'disable' : 'enable', { instance_id: instance.id })
                    }
                  >
                    <ActionIcon name={instance.enabled ? 'disableTask' : 'enableTask'} />
                  </IconButton>
                )}
                {admin && (
                  <PluginPackageActions
                    instance={instance}
                    canSetDefault={
                      instance.is_default === false &&
                      instance.enabled &&
                      Object.keys(instance.declaration.agents ?? {}).length > 0
                    }
                    upgradeGrants={
                      catalog.data?.packages.find((p) => p.source === instance.source)?.extensions[
                        instance.extension
                      ]?.permissions
                    }
                    allowInstall={catalog.data?.remote_management}
                    disabled={!runner.online || busy}
                    mutate={mutate}
                  />
                )}
              </div>
            </div>
            <DetailTabs
              idPrefix={`plugin-${instance.id}`}
              label={tr('plugins.title')}
              tabs={[
                { key: 'config', label: tr('plugins.config') },
                { key: 'capabilities', label: tr('plugins.capabilities') },
                { key: 'activity', label: tr('plugins.activity') },
              ]}
              value={tab}
              onChange={setTab}
            />
            <div className="plugin-detail-body">
              {tab === 'config' && (!runner.online || !admin) ? (
                <p className="p-4 text-muted">
                  {tr(!runner.online ? 'plugins.offlineSettings' : 'plugins.adminSettings')}
                </p>
              ) : tab === 'config' ? (
                <PluginConfig
                  key={`${instance.id}:${instance.revision}`}
                  instance={instance}
                  disabled={!admin || !runner.online || busy}
                  onSave={(update) => mutate('configure', { instance_id: instance.id, update })}
                />
              ) : tab === 'capabilities' ? (
                <PluginCapabilities
                  instance={instance}
                  runner={runner}
                  admin={admin}
                  toast={toast}
                />
              ) : (
                <PluginActivity
                  canAct={canAct}
                  instance={instance}
                  control={control}
                  mutate={mutate}
                  admin={admin}
                  online={runner.online}
                  toast={toast}
                />
              )}
            </div>
          </>
        ) : (
          <div className="empty">
            <p>{tr('plugins.select')}</p>
          </div>
        )}
      </div>
      <PluginCreateDialog
        resources={resources}
        open={creating}
        onOpenChange={(open) => {
          setCreating(open)
          if (!open) {
            setupSession.current = { id: crypto.randomUUID() }
            setSetupSaved(false)
            setSetupLocked(false)
          }
        }}
        pending={busy}
        loading={catalog.isPending}
        offline={!runner.online}
        loadError={catalog.error?.message}
        setupSaved={setupSaved}
        setupLocked={setupLocked}
        onSubmit={async (draft: PluginSetupDraft) => {
          setBusy(true)
          setSetupLocked(true)
          try {
            const result = await submitPluginSetup(
              control,
              setupSession.current,
              draft,
              (created) => {
                setSelected(created.id)
                setSetupSaved(true)
              },
            )
            setSelected(result.id)
            toast(tr('已保存'))
          } catch (error) {
            if (error instanceof PluginSetupError)
              throw new Error(tr(`pluginCreate.error.${error.code}`))
            throw error
          } finally {
            setBusy(false)
            await refresh()
          }
        }}
      />
    </div>
  )
}
function PluginConfig({
  instance,
  disabled,
  onSave,
}: {
  instance: Instance
  disabled: boolean
  onSave: (update: unknown) => Promise<unknown>
}) {
  const [value, setValue] = useState<unknown>(instance.config ?? {})
  const saveReceipt = useRef<{ fingerprint: string; id: string } | null>(null)
  const [secrets, setSecrets] = useState<
    Record<string, { operation: 'keep' | 'replace' | 'clear'; value?: string }>
  >({})
  const save = () => {
    const fingerprint = JSON.stringify({ revision: instance.revision, config: value, secrets })
    if (saveReceipt.current?.fingerprint !== fingerprint)
      saveReceipt.current = { fingerprint, id: crypto.randomUUID() }
    return onSave({
      operation_id: saveReceipt.current.id,
      expected_revision: instance.revision,
      config: value,
      secrets,
    })
  }
  return (
    <div className="plugin-settings">
      <div className="plugin-config-card">
        <div className="plugin-section-title">{tr('plugins.config')}</div>
        <PluginFields
          schema={instance.declaration.config}
          value={value}
          disabled={disabled}
          onChange={setValue}
        />
        {!!Object.keys(instance.declaration.credentials).length && (
          <>
            <div className="plugin-section-title">{tr('plugins.credentials')}</div>
            {Object.entries(instance.declaration.credentials).map(([slot, spec]) => (
              <div className="plugin-field" key={slot}>
                <div className="plugin-field-label">
                  {spec.label}
                  {spec.required && ' *'}
                </div>
                <div className="plugin-secret-control">
                  {secrets[slot]?.operation === 'replace' ? (
                    <input
                      type="password"
                      autoComplete="new-password"
                      aria-label={spec.label}
                      value={secrets[slot].value ?? ''}
                      disabled={disabled}
                      onChange={(e) =>
                        setSecrets({
                          ...secrets,
                          [slot]: { operation: 'replace', value: e.target.value },
                        })
                      }
                    />
                  ) : (
                    <span className="text-muted">
                      {secrets[slot]?.operation === 'clear'
                        ? tr('plugins.willClear')
                        : instance.credential_status[slot]
                          ? tr('plugins.configured')
                          : tr('plugins.notConfigured')}
                    </span>
                  )}
                  <div className="action-group">
                    <IconButton
                      label={tr('plugins.replaceCredential', { name: spec.label })}
                      disabled={disabled}
                      onClick={() =>
                        setSecrets({ ...secrets, [slot]: { operation: 'replace', value: '' } })
                      }
                    >
                      <ActionIcon name="edit" />
                    </IconButton>
                    <IconButton
                      label={tr('plugins.clearCredential', { name: spec.label })}
                      danger
                      disabled={disabled || !instance.credential_status[slot]}
                      onClick={() => setSecrets({ ...secrets, [slot]: { operation: 'clear' } })}
                    >
                      <ActionIcon name="delete" />
                    </IconButton>
                    {secrets[slot] && secrets[slot].operation !== 'keep' && (
                      <IconButton
                        label={tr('plugins.undoCredential', { name: spec.label })}
                        disabled={disabled}
                        onClick={() => setSecrets({ ...secrets, [slot]: { operation: 'keep' } })}
                      >
                        <ActionIcon name="undo" />
                      </IconButton>
                    )}
                  </div>
                </div>
              </div>
            ))}
          </>
        )}
        <div className="plugin-config-footer">
          <span className="text-muted">
            {!instance.enabled
              ? tr('plugins.state.disabled')
              : instance.applied_revision === instance.revision
                ? tr('plugins.applied')
                : tr('plugins.pending')}
          </span>
          <Button variant="solid" disabled={disabled} onClick={save}>
            {tr('保存')}
          </Button>
        </div>
      </div>
    </div>
  )
}
function PluginCapabilities({
  instance,
  runner,
  admin,
  toast,
}: {
  instance: Instance
  runner: Runner
  admin: boolean
  toast: (s: string) => void
}) {
  const client = useQueryClient()
  const [grants, setGrants] = useState(instance.grants)
  useEffect(() => setGrants(instance.grants), [instance.id, instance.revision])
  const saveGrants = async () => {
    try {
      await api(`/runners/${runner.id}/plugins/control`, 'POST', {
        action: 'authorize',
        params: { instance_id: instance.id, grants },
      })
      await client.invalidateQueries({ queryKey: ['plugins', runner.id] })
      toast(tr('已保存'))
    } catch (error) {
      toast((error as Error).message)
    }
  }

  const bindings = useQuery<{ items: Binding[] }>({
    queryKey: ['plugin-bindings', instance.id],
    queryFn: () => api(`/plugins/bindings?instance=${instance.id}`),
  })
  const declarations = [
    ...Object.entries(instance.declaration.handlers).flatMap(([handler, spec]) =>
      spec.events.map((event) => ({
        kind: 'event' as const,
        handler,
        event,
        description: spec.description,
      })),
    ),
    ...Object.entries(instance.declaration.hooks).map(([handler, spec]) => ({
      kind: 'hook' as const,
      handler,
      event: spec.stage,
      description: spec.description,
    })),
  ]
  const save = async (binding: Binding) => {
    try {
      await api('/plugins/bindings', 'POST', binding)
      await client.invalidateQueries({ queryKey: ['plugin-bindings', instance.id] })
      toast(tr('已保存'))
    } catch (e) {
      toast((e as Error).message)
    }
  }
  return (
    <div className="plugin-capabilities">
      <div className="plugin-section-title">{tr('plugins.eventsHooks')}</div>
      {bindings.error && <ErrorToast error={bindings.error.message} />}
      {declarations.map((d) => {
        const b = bindings.data?.items.find((b) => b.handler === d.handler && b.event === d.event)
        const candidate: Binding = b ?? {
          id: crypto.randomUUID(),
          instance_id: instance.id,
          runner_id: runner.id,
          kind: d.kind,
          event: d.event,
          handler: d.handler,
          enabled: false,
          filter: {},
          position: 0,
          failure_policy: 'reject',
          timeout_seconds: 30,
          max_attempts: 5,
          expires_seconds: 604800,
          revision: 0,
        }
        return (
          <div className="plugin-capability" key={`${d.kind}:${d.handler}:${d.event}`}>
            <div>
              <strong>{d.event}</strong>
              <span className="text-muted">{d.description}</span>
            </div>
            <div className="plugin-capability-controls">
              {d.kind === 'hook' &&
              /^(execution|session|result|artifact|action)\./.test(d.event) ? (
                <span className="text-muted">{tr('plugins.device')}</span>
              ) : (
                <ChoicePicker
                  label={tr('plugins.scope')}
                  value={candidate.filter.runner_id ? 'device' : 'workspace'}
                  disabled={!admin}
                  options={[
                    { value: 'workspace', label: tr('plugins.workspace') },
                    { value: 'device', label: tr('plugins.device') },
                  ]}
                  onChange={(v) =>
                    save({ ...candidate, filter: v === 'device' ? { runner_id: runner.id } : {} })
                  }
                />
              )}
              {d.kind === 'hook' && (
                <>
                  <input
                    type="number"
                    className="plugin-order"
                    aria-label={tr('plugins.order')}
                    title={tr('plugins.order')}
                    key={`${candidate.id}:${candidate.revision}`}
                    defaultValue={candidate.position}
                    disabled={!admin}
                    onBlur={(e) =>
                      Number(e.target.value) !== candidate.position &&
                      save({ ...candidate, position: Number(e.target.value) })
                    }
                  />
                  <ChoicePicker
                    label={tr('plugins.failurePolicy')}
                    value={candidate.failure_policy}
                    disabled={!admin}
                    options={['reject', 'wait', 'continue'].map((p) => ({
                      value: p,
                      label: tr(`plugins.policy.${p}`),
                    }))}
                    onChange={(value) => save({ ...candidate, failure_policy: value })}
                  />
                </>
              )}
              <IconButton
                className="ml-auto"
                label={tr(candidate.enabled ? 'plugins.disable' : 'plugins.enable', {
                  name: d.event,
                })}
                disabled={!admin}
                onClick={() => save({ ...candidate, enabled: !candidate.enabled })}
              >
                <ActionIcon name={candidate.enabled ? 'disableTask' : 'enableTask'} />
              </IconButton>
            </div>
          </div>
        )
      })}
      <div className="plugin-section-title">{tr('plugins.grantedPermissions')}</div>
      {instance.declaration.permissions.map((grant) => (
        <label className="plugin-permission flex gap-2" key={grant}>
          <input
            type="checkbox"
            disabled={!admin || !runner.online}
            checked={grants.includes(grant)}
            onChange={(event) =>
              setGrants(
                event.target.checked
                  ? [...grants, grant]
                  : grants.filter((value) => value !== grant),
              )
            }
          />
          {grant}
        </label>
      ))}
      {admin && (
        <div className="plugin-config-footer">
          <span />
          <Button
            variant="solid"
            disabled={!runner.online || JSON.stringify(grants) === JSON.stringify(instance.grants)}
            onClick={saveGrants}
          >
            {tr('保存')}
          </Button>
        </div>
      )}
    </div>
  )
}
function PluginActivity({
  instance,
  control,
  mutate,
  admin,
  canAct,
  online,
  toast,
}: {
  instance: Instance
  control: (action: string, params?: Record<string, unknown>) => Promise<any>
  mutate: (action: string, params: Record<string, unknown>) => Promise<any>
  admin: boolean
  canAct: boolean
  online: boolean
  toast: (s: string) => void
}) {
  const [action, setAction] = useState('')
  const [input, setInput] = useState<unknown>({})
  const [interaction, setInteraction] = useState<any>(null)
  const [dismissed, setDismissed] = useState<string[]>([])
  const seenNotifications = useRef<Set<string> | null>(null)
  const [answer, setAnswer] = useState<unknown>({})
  const activity = useQuery<any>({
    queryKey: ['plugin-activity', instance.id],
    queryFn: () => control('activity', { instance_id: instance.id }),
    enabled: online,
    refetchInterval: 1500,
  })
  const deliveries = useQuery<{ items: any[] }>({
    queryKey: ['plugin-deliveries', instance.id],
    queryFn: () => api(`/plugins/deliveries?instance=${instance.id}`),
    refetchInterval: 5000,
  })
  useEffect(() => {
    const pending = activity.data?.interactions?.find((item: any) => !dismissed.includes(item.id))
    if (pending && pending.id !== interaction?.id) {
      setInteraction(pending)
      setAnswer({})
    }
  }, [activity.data, interaction?.id, dismissed])
  useEffect(() => {
    const notifications = activity.data?.notifications ?? []
    if (!seenNotifications.current) {
      seenNotifications.current = new Set(notifications.map((n: any) => n.id))
      return
    }
    for (const notification of notifications) {
      if (!seenNotifications.current.has(notification.id) && notification.request.kind === 'toast')
        toast(notification.request.message)
      seenNotifications.current.add(notification.id)
    }
  }, [activity.data?.notifications, toast])
  const execute = async () => {
    const id = crypto.randomUUID()
    await mutate('invoke', {
      invocation: {
        id,
        operation_id: id,
        instance_id: instance.id,
        kind: 'action',
        handler: action,
        input,
        deadline: new Date(Date.now() + 3600000).toISOString(),
        expected_revision: instance.revision,
        actor: { kind: 'user', id: null, roles: [] },
        causation_id: null,
      },
    })
    setAction('')
  }
  return (
    <div className="plugin-activity">
      {(activity.data?.interactions ?? [])
        .filter((item: any) => dismissed.includes(item.id))
        .map((item: any) => (
          <Button
            key={item.id}
            onClick={() => {
              setDismissed(dismissed.filter((id) => id !== item.id))
              setInteraction(item)
            }}
          >
            {tr('plugins.continueInteraction')}
          </Button>
        ))}
      {(activity.data?.notifications ?? []).map((item: any) => (
        <div
          className="plugin-activity-row"
          key={item.id}
          aria-live={item.request.kind === 'progress' ? 'polite' : undefined}
        >
          <span>{item.request.message}</span>
          {item.request.kind === 'progress' && typeof item.request.fraction === 'number' && (
            <progress max={1} value={item.request.fraction} />
          )}
          {item.request.kind === 'navigate' &&
            /^\/(tasks|runs|runners|presets|settings)([/?]|$)/.test(item.request.path) && (
              <a href={workspaceBase + item.request.path}>{tr('plugins.open')}</a>
            )}
          {item.request.kind === 'download' && /^[0-9a-f-]{36}$/i.test(item.request.reference) && (
            <a href={apiUrl(`/plugins/artifacts/${item.request.reference}/download`)}>
              {item.request.name}
            </a>
          )}
        </div>
      ))}

      <div className="plugin-section-title">{tr('plugins.actions')}</div>
      <div className="plugin-actions">
        {Object.entries(instance.declaration.actions).map(([id, a]) => (
          <Button
            disabled={!canAct || !online}
            key={id}
            onClick={() => {
              setAction(id)
              setInput({})
            }}
          >
            {a.label}
          </Button>
        ))}
      </div>
      {activity.error && <ErrorToast error={activity.error.message} />}
      {(activity.data?.items ?? []).map((item: any) => (
        <div className="plugin-activity-row" key={item.id}>
          <span>{item.handler}</span>
          <span className="text-muted">{item.state}</span>
          <span className="text-muted">{item.error}</span>
          {item.state === 'running' && admin && (
            <IconButton
              label={tr('plugins.cancelInvocation', { name: item.handler })}
              onClick={() => mutate('cancel', { instance_id: instance.id, invocation_id: item.id })}
            >
              <ActionIcon name="stop" />
            </IconButton>
          )}
        </div>
      ))}
      {!!activity.data?.logs?.length && (
        <>
          <div className="plugin-section-title">{tr('plugins.logs')}</div>
          <div className="plugin-diagnostics">
            {activity.data.logs.map((entry: any, index: number) => (
              <pre key={index}>{entry.message ?? JSON.stringify(entry.data ?? entry)}</pre>
            ))}
          </div>
        </>
      )}
      <div className="plugin-section-title">{tr('plugins.deliveries')}</div>
      {deliveries.data?.items.map((d) => (
        <div className="plugin-activity-row" key={d.id}>
          <span>{d.state}</span>
          <span className="text-muted">{d.error}</span>
          {admin && (
            <div className="action-group">
              <IconButton
                label={tr('plugins.retryDelivery')}
                onClick={async () => {
                  try {
                    await api(`/plugins/deliveries/${d.id}`, 'POST', { action: 'retry' })
                    await deliveries.refetch()
                  } catch (e) {
                    toast((e as Error).message)
                  }
                }}
              >
                <ActionIcon name="retry" />
              </IconButton>
              <ActionMenu label={tr('plugins.deliveryActions')}>
                <ActionMenuItem
                  icon="rerun"
                  reason={tr('plugins.replayDescription')}
                  onSelect={async () => {
                    try {
                      await api(`/plugins/deliveries/${d.id}`, 'POST', { action: 'replay' })
                      await deliveries.refetch()
                    } catch (e) {
                      toast((e as Error).message)
                    }
                  }}
                >
                  {tr('plugins.replay')}
                </ActionMenuItem>
              </ActionMenu>
            </div>
          )}
        </div>
      ))}
      <Modal
        open={!!action}
        onOpenChange={(v) => !v && setAction('')}
        title={instance.declaration.actions[action]?.label ?? ''}
        actions={
          <>
            <Button onClick={() => setAction('')}>{tr('取消')}</Button>
            <Button variant="solid" onClick={execute}>
              {tr('执行')}
            </Button>
          </>
        }
      >
        <div className="p-5">
          <PluginFields
            schema={instance.declaration.actions[action]?.input ?? {}}
            value={input}
            onChange={setInput}
          />
        </div>
      </Modal>
      <Modal
        open={!!interaction}
        onOpenChange={(v) => {
          if (!v && interaction) {
            setDismissed([...dismissed, interaction.id])
            setInteraction(null)
          }
        }}
        title={
          interaction?.request.title ?? interaction?.request.label ?? tr('plugins.interaction')
        }
        actions={
          <>
            <Button onClick={() => setInteraction(null)}>{tr('关闭')}</Button>
            <Button
              variant="solid"
              onClick={async () => {
                await mutate('interaction', {
                  instance_id: instance.id,
                  id: interaction.id,
                  response: interaction.request.kind === 'confirm' ? true : answer,
                })
                setInteraction(null)
                await activity.refetch()
              }}
            >
              {tr('确认')}
            </Button>
          </>
        }
      >
        <div className="p-5">
          {interaction?.request.kind === 'form' ? (
            <PluginFields schema={interaction.request.schema} value={answer} onChange={setAnswer} />
          ) : interaction?.request.kind === 'secret' ? (
            <input
              type="password"
              autoComplete="new-password"
              value={typeof answer === 'string' ? answer : ''}
              onChange={(e) => setAnswer(e.target.value)}
            />
          ) : interaction?.request.kind === 'choice' ? (
            <ChoicePicker
              label={interaction.request.title}
              value={String(answer)}
              options={interaction.request.options}
              onChange={setAnswer}
            />
          ) : (
            <>
              <p>{interaction?.request.description ?? interaction?.request.message}</p>
              {interaction?.request.url && (
                <a href={interaction.request.url} target="_blank" rel="noreferrer">
                  {interaction.request.label ?? tr('plugins.authorize')}
                </a>
              )}
              {interaction?.request.code && <code>{interaction.request.code}</code>}
            </>
          )}
        </div>
      </Modal>
    </div>
  )
}

function localized(declaration: Declaration, language: string): Declaration {
  const copy = structuredClone(declaration)
  for (const [path, value] of Object.entries(
    declaration.translations?.[language.split('-')[0]] ?? {},
  )) {
    const parts = path.split('.')
    if (parts.some((part) => ['__proto__', 'constructor', 'prototype'].includes(part))) continue
    let target: any = copy
    for (const key of parts.slice(0, -1)) {
      if (!target || typeof target !== 'object') break
      target = target[key]
    }
    if (target && typeof target === 'object') target[parts.at(-1)!] = value
  }
  return copy
}
