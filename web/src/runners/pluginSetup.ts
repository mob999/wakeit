export type PluginSetupDraft = {
  resourceValue: string
  name: string
  config: unknown
  grants: string[]
  secrets: Record<string, { operation: 'replace'; value: string }>
  enable: boolean
}
export type SetupInstance = {
  id: string
  revision: number
  enabled: boolean
  state: string
}
export type PluginSetupSession = {
  id: string
  identity?: string
  creation?: Record<string, unknown>
  instance?: SetupInstance
  configured?: string
  update?: { signature: string; operation_id: string; expected_revision: number }
  activationAttempted?: boolean
  busy?: boolean
}
export class PluginSetupError extends Error {
  code: 'selection_locked' | 'activation_pending' | 'receipt_pending' | 'busy'
  constructor(code: PluginSetupError['code']) {
    super(code)
    this.code = code
  }
}
type Control = (action: string, params: Record<string, unknown>) => Promise<any>

/** Keep one creation identity and one receipt per logical settings update across retries. */
export async function submitPluginSetup(
  control: Control,
  session: PluginSetupSession,
  draft: PluginSetupDraft,
  onCreated: (instance: SetupInstance) => void,
): Promise<SetupInstance> {
  if (session.busy) throw new PluginSetupError('busy')
  const identity = JSON.stringify([draft.resourceValue, draft.name, [...draft.grants].sort()])
  if (session.identity && identity !== session.identity)
    throw new PluginSetupError('selection_locked')
  const signature = JSON.stringify([draft.config, draft.secrets])
  session.busy = true
  try {
    if (session.activationAttempted) {
      let catalog
      try {
        catalog = await control('catalog', {})
      } catch {
        throw new PluginSetupError('activation_pending')
      }
      const current = catalog.instances?.find((i: SetupInstance) => i.id === session.id)
      if (!current) throw new PluginSetupError('activation_pending')
      session.instance = current
      if (!current.enabled) session.activationAttempted = false
      else if (!draft.enable) {
        try {
          session.instance = await control('disable', { instance_id: session.id })
          session.activationAttempted = false
        } catch {
          throw new PluginSetupError('activation_pending')
        }
      } else if (signature === session.configured && draft.enable && current.state === 'running')
        return current
      else throw new PluginSetupError('activation_pending')
    }
    if (!session.instance) {
      const [source, extension] = JSON.parse(draft.resourceValue) as [string, string]
      session.identity = identity
      session.creation ??= {
        instance_id: session.id,
        source,
        extension,
        name: draft.name,
        grants: [...draft.grants],
        config: draft.config,
      }
      session.instance = await control('create', session.creation)
    }
    onCreated(session.instance!)
    if (signature !== session.configured) {
      // If an earlier response was lost, resolve that receipt before changing its payload.
      if (session.update && session.update.signature !== signature) {
        const previous = await control('operation', {
          instance_id: session.id,
          operation_id: session.update.operation_id,
        })
        if (previous.state === 'committed') session.instance!.revision = previous.revision
        else if (previous.state !== 'not_found') throw new PluginSetupError('receipt_pending')
      }
      if (session.update?.signature !== signature)
        session.update = {
          signature,
          operation_id: crypto.randomUUID(),
          expected_revision: session.instance!.revision,
        }
      const receipt = await control('configure', {
        instance_id: session.id,
        update: {
          operation_id: session.update.operation_id,
          expected_revision: session.update.expected_revision,
          config: draft.config,
          secrets: draft.secrets,
        },
      })
      session.instance!.revision = receipt.revision
      session.configured = signature
      session.update = undefined
    }
    if (draft.enable) {
      session.activationAttempted = true
      try {
        session.instance = await control('enable', { instance_id: session.id })
      } catch (error) {
        // enable may have persisted its flag before process startup failed.
        // Keep the saved settings, but undo this new instance's activation when confirmed.
        try {
          session.instance = await control('disable', { instance_id: session.id })
          session.activationAttempted = false
        } catch {
          throw new PluginSetupError('activation_pending')
        }
        throw error
      }
    } else if (session.activationAttempted) {
      session.instance = await control('disable', { instance_id: session.id })
      session.activationAttempted = false
    }
    return session.instance!
  } finally {
    session.busy = false
  }
}
