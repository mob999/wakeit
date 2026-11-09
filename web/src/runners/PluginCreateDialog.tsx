import { useEffect, useId, useRef, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { t as tr } from '../i18n'
import { ChoicePicker } from '../components/ChoicePicker'
import { ContentCard } from '../components/ContentCard'
import { Button, Field, Modal } from '../components/ui'
import { ActionIcon } from '../actions/ActionIcon'
import { InfoTrigger } from '../actions/InfoTrigger'
import { PluginFields, schemaDefaults, type Schema } from './PluginFields'
import './plugins.css'
import './PluginCreateDialog.css'

export type PluginCreateResource = {
  value: string
  label: string
  packageName?: string
  description?: string
  version: string
  agents?: readonly string[]
  events?: readonly string[]
  hooks?: readonly string[]
  actions?: readonly string[]
  permissions: readonly string[]
  config?: Schema
  credentials?: Record<string, { label: string; description?: string; required?: boolean }>
}

export type PluginCreateSubmission = {
  resourceValue: string
  name: string
  grants: string[]
  config: unknown
  secrets: Record<string, { operation: 'replace'; value: string }>
  enable: boolean
}

export type PluginCreateDialogProps = {
  resources: readonly PluginCreateResource[]
  open: boolean
  onOpenChange: (open: boolean) => void
  pending?: boolean
  loading?: boolean
  offline?: boolean
  loadError?: string | null
  setupSaved?: boolean
  setupLocked?: boolean
  onSubmit: (submission: PluginCreateSubmission) => Promise<unknown>
}

const permissionKeys: Record<string, string> = {
  'events:read': 'events',
  'tasks:read': 'tasksRead',
  'tasks:write': 'tasksWrite',
  'executions:read': 'executions',
  'results:read': 'results',
  'files:read': 'filesRead',
  'files:watch': 'filesWatch',
  'artifacts:read': 'artifactsRead',
  'artifacts:write': 'artifactsWrite',
  'http:request': 'http',
  'process:execute': 'process',
  'auth:manage': 'auth',
  'hooks:execution.prepare': 'executionHook',
  'hooks:task.save.prepare': 'taskHook',
  'hooks:result.publish.prepare': 'resultHook',
}

function PluginCreateInfo({ label, description }: { label: string; description: string }) {
  return (
    <InfoTrigger
      label={`${label} — ${tr('详情')}`}
      tooltip={description}
      className="plugin-create-info"
      content={<p>{description}</p>}
    >
      <ActionIcon name="info" />
    </InfoTrigger>
  )
}

function hasConfigFields(schema?: Schema): boolean {
  if (!schema) return false
  const types = Array.isArray(schema.type) ? schema.type : [schema.type]
  return Boolean(
    Object.keys(schema.properties ?? {}).length ||
    types.some((type) => type && type !== 'object' && type !== 'null') ||
    schema.$ref ||
    schema.enum?.length ||
    schema.oneOf?.length ||
    schema.anyOf?.length ||
    schema.allOf?.some(hasConfigFields) ||
    hasConfigFields(schema.then) ||
    hasConfigFields(schema.else),
  )
}

export function PluginCreateDialog({
  resources,
  open,
  onOpenChange,
  pending = false,
  loading = false,
  offline = false,
  loadError,
  setupSaved = false,
  setupLocked = setupSaved,
  onSubmit,
}: PluginCreateDialogProps) {
  useTranslation()
  const [resource, setResource] = useState<PluginCreateResource | null>(null)
  const [name, setName] = useState('')
  const [grants, setGrants] = useState<string[]>([])
  const [config, setConfig] = useState<unknown>({})
  const [secrets, setSecrets] = useState<Record<string, string>>({})
  const [enable, setEnable] = useState(false)
  const [attempted, setAttempted] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [submitError, setSubmitError] = useState<string | null>(null)
  const credentialId = useId()
  const submitLock = useRef(false)
  const errorRef = useRef<HTMLDivElement>(null)
  const busy = pending || submitting
  const identityLocked = setupLocked || setupSaved || attempted
  const permissions = [...new Set(resource?.permissions ?? [])]
  const credentials = Object.entries(resource?.credentials ?? {})
  const available =
    resource && (identityLocked || resources.some((r) => r.value === resource.value))
  const canSubmit =
    available && name.trim() && !busy && !offline && (identityLocked || (!loading && !loadError))

  useEffect(() => {
    if (open) return
    setResource(null)
    setName('')
    setGrants([])
    setConfig({})
    setSecrets({})
    setEnable(false)
    setAttempted(false)
    setSubmitError(null)
  }, [open])
  useEffect(() => {
    if (submitError !== null) errorRef.current?.focus()
  }, [submitError])

  function changeOpen(next: boolean) {
    if (!pending && !submitLock.current) onOpenChange(next)
  }
  function selectResource(value: string) {
    if (busy || identityLocked || resource?.value === value) return
    const next = resources.find((item) => item.value === value) ?? null
    setResource(next)
    setName(next?.label ?? '')
    setGrants([])
    setConfig(next?.config ? (schemaDefaults(next.config) ?? {}) : {})
    setSecrets({})
    setEnable(false)
    setSubmitError(null)
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!canSubmit || !resource || submitLock.current) return
    submitLock.current = true
    setSubmitting(true)
    setAttempted(true)
    setSubmitError(null)
    try {
      await onSubmit({
        resourceValue: resource.value,
        name: name.trim(),
        grants: permissions.filter((permission) => grants.includes(permission)),
        config,
        secrets: Object.fromEntries(
          credentials.flatMap(([slot]) =>
            secrets[slot] ? [[slot, { operation: 'replace' as const, value: secrets[slot] }]] : [],
          ),
        ),
        enable,
      })
      onOpenChange(false)
    } catch (error) {
      setSubmitError(error instanceof Error ? error.message : '')
    } finally {
      submitLock.current = false
      setSubmitting(false)
    }
  }

  return (
    <Modal
      open={open}
      onOpenChange={changeOpen}
      title={tr('pluginCreate.title')}
      className={`plugin-create-dialog${busy ? ' is-pending' : ''}`}
      closePending={busy}
    >
      <form onSubmit={submit} aria-busy={busy || undefined}>
        <div className="modal-body plugin-create-body">
          {setupSaved && !busy ? (
            <p className="plugin-create-notice" role="status">
              {tr('pluginCreate.setupSaved')}
            </p>
          ) : identityLocked && !busy ? (
            <p className="plugin-create-notice" role="status">
              {tr('pluginCreate.setupLocked')}
            </p>
          ) : null}
          {offline ? (
            <p className="plugin-create-notice" role="status">
              {tr('pluginCreate.offline')}
            </p>
          ) : loading ? (
            <p className="plugin-create-notice" role="status">
              {tr('pluginCreate.loading')}
            </p>
          ) : !resources.length && !loadError && !identityLocked ? (
            <p className="plugin-create-notice" role="status">
              {tr('pluginCreate.noResources')}
            </p>
          ) : null}
          {loadError && (
            <div className="plugin-create-error" role="alert">
              <strong>{tr('pluginCreate.loadFailed')}</strong>
              <span>{loadError}</span>
            </div>
          )}
          {resource && !available && (
            <p className="plugin-create-notice" role="status">
              {tr('pluginCreate.unavailable')}
            </p>
          )}
          <ContentCard
            title={tr('pluginCreate.details')}
            className="plugin-create-details"
            actions={
              resource?.description && (
                <PluginCreateInfo
                  label={tr('pluginCreate.details')}
                  description={resource.description}
                />
              )
            }
          >
            <Field label={tr('plugins.resource')}>
              <ChoicePicker
                label={tr('plugins.resource')}
                value={resource?.value ?? ''}
                selectionLabel={resource?.label}
                options={resources.map((item) => ({
                  value: item.value,
                  label: item.label,
                  description: item.packageName ?? item.description,
                  keywords: [item.packageName, item.description, ...(item.agents ?? [])].join(' '),
                }))}
                placeholder={tr('pluginCreate.choose')}
                searchable
                disabled={busy || identityLocked || loading || !!loadError || !resources.length}
                onChange={selectResource}
              />
            </Field>
            <Field label={tr('名称')}>
              <input
                value={name}
                required
                autoComplete="off"
                placeholder={resource?.label ?? tr('pluginCreate.chooseFirst')}
                disabled={busy || identityLocked || !resource}
                onChange={(event) => setName(event.target.value)}
              />
            </Field>
            {resource && (
              <>
                <dl className="properties plugin-create-properties">
                  <dt>{tr('pluginCreate.package')}</dt>
                  <dd>
                    {resource.packageName || '—'}
                    <span className="plugin-create-version">{resource.version}</span>
                  </dd>
                  <dt>{tr('pluginCreate.capabilities')}</dt>
                  <dd className="plugin-create-capabilities">
                    {!!resource.agents?.length && (
                      <span>
                        {tr('pluginCreate.agents')}: {resource.agents.join(', ')}
                      </span>
                    )}
                    {!!resource.events?.length && (
                      <span>
                        {tr('pluginCreate.events', { count: new Set(resource.events).size })}
                      </span>
                    )}
                    {!!resource.hooks?.length && (
                      <span>
                        {tr('pluginCreate.hooks', { count: new Set(resource.hooks).size })}
                      </span>
                    )}
                    {!!resource.actions?.length && (
                      <span>{tr('pluginCreate.actions', { count: resource.actions.length })}</span>
                    )}
                    {!resource.agents?.length &&
                      !resource.events?.length &&
                      !resource.hooks?.length &&
                      !resource.actions?.length &&
                      '—'}
                  </dd>
                </dl>
              </>
            )}
          </ContentCard>
          {resource?.config && hasConfigFields(resource.config) && (
            <ContentCard title={tr('plugins.config')} className="plugin-create-config">
              <PluginFields
                key={resource.value}
                schema={resource.config}
                value={config}
                disabled={busy}
                onChange={setConfig}
              />
            </ContentCard>
          )}
          {!!credentials.length && (
            <ContentCard
              title={tr('plugins.credentials')}
              className="plugin-create-credentials"
              actions={
                <PluginCreateInfo
                  label={tr('plugins.credentials')}
                  description={tr('pluginCreate.credentialsHint')}
                />
              }
            >
              {credentials.map(([slot, spec], index) => (
                <div className="field" key={slot}>
                  <div className="plugin-create-field-label">
                    <label htmlFor={`${credentialId}-${index}`}>
                      {spec.label}
                      {spec.required ? ' *' : ''}
                    </label>
                    {spec.description && (
                      <PluginCreateInfo label={spec.label} description={spec.description} />
                    )}
                  </div>
                  <input
                    id={`${credentialId}-${index}`}
                    type="password"
                    required={!!spec.required && !setupSaved}
                    autoComplete="new-password"
                    spellCheck={false}
                    value={secrets[slot] ?? ''}
                    disabled={busy}
                    onChange={(event) => setSecrets({ ...secrets, [slot]: event.target.value })}
                  />
                </div>
              ))}
            </ContentCard>
          )}
          {resource && (
            <ContentCard
              title={tr('pluginCreate.permissions')}
              className="plugin-create-permissions"
              actions={
                !!permissions.length && (
                  <PluginCreateInfo
                    label={tr('pluginCreate.permissions')}
                    description={`${tr('pluginCreate.permissionsHint')}\n\n${tr('pluginCreate.permissionBoundary')}`}
                  />
                )
              }
            >
              {!permissions.length && (
                <p className="plugin-create-empty">{tr('pluginCreate.noPermissions')}</p>
              )}
              {permissions.map((permission) => {
                const key = `pluginCreate.permission.${
                  Object.hasOwn(permissionKeys, permission) ? permissionKeys[permission] : 'unknown'
                }`
                return (
                  <div className="plugin-create-permission" key={permission}>
                    <label className="plugin-create-permission-choice">
                      <input
                        type="checkbox"
                        checked={grants.includes(permission)}
                        disabled={busy || identityLocked}
                        onChange={(event) =>
                          setGrants((current) =>
                            event.target.checked
                              ? [...current, permission]
                              : current.filter((item) => item !== permission),
                          )
                        }
                      />
                      <span className="plugin-create-permission-heading">
                        <span>{tr(`${key}.title`)}</span>
                        <code>{permission}</code>
                      </span>
                    </label>
                    <PluginCreateInfo
                      label={tr(`${key}.title`)}
                      description={tr(`${key}.description`)}
                    />
                  </div>
                )
              })}
            </ContentCard>
          )}
          {submitError !== null && (
            <div className="plugin-create-error" role="alert" tabIndex={-1} ref={errorRef}>
              <strong>{tr('pluginCreate.submitFailed')}</strong>
              {submitError && <span>{submitError}</span>}
            </div>
          )}
        </div>
        <div className="modal-footer plugin-create-footer">
          <label className="plugin-create-enable">
            <input
              type="checkbox"
              checked={enable}
              disabled={busy || !resource}
              onChange={(event) => setEnable(event.target.checked)}
            />
            <span>{tr('pluginCreate.enable')}</span>
          </label>
          <div className="plugin-create-actions">
            <Button disabled={busy} onClick={() => changeOpen(false)}>
              {tr('取消')}
            </Button>
            <Button type="submit" variant="solid" disabled={!canSubmit} loading={busy}>
              {tr(
                busy
                  ? 'pluginCreate.saving'
                  : identityLocked
                    ? 'pluginCreate.retry'
                    : 'pluginCreate.title',
              )}
            </Button>
          </div>
        </div>
      </form>
    </Modal>
  )
}
