import test from 'node:test'
import assert from 'node:assert/strict'
import {
  PluginSetupError,
  submitPluginSetup,
  type PluginSetupDraft,
  type PluginSetupSession,
} from '../../src/runners/pluginSetup.ts'
const draft = (): PluginSetupDraft => ({
  resourceValue: JSON.stringify(['local:synthetic', 'mail']),
  name: 'Demo mail',
  config: { host: 'smtp.example.test' },
  grants: ['events:read'],
  secrets: { account: { operation: 'replace', value: 'synthetic-only' } },
  enable: false,
})
const session = (): PluginSetupSession => ({ id: crypto.randomUUID() })
const instance = (s: PluginSetupSession, revision = 1) => ({
  id: s.id,
  revision,
  enabled: false,
  state: 'disabled',
})

test('settings and secrets are saved before an explicitly requested activation', async () => {
  const s = session(),
    d = { ...draft(), enable: true }
  const calls: string[] = []
  await submitPluginSetup(
    async (action, params) => {
      calls.push(action)
      if (action === 'create') {
        assert.equal(params.instance_id, s.id)
        assert.deepEqual(params.grants, ['events:read'])
        assert.equal('secrets' in params, false)
        return instance(s)
      }
      if (action === 'configure') {
        const update = params.update as any
        assert.equal(update.expected_revision, 1)
        assert.deepEqual(update.secrets, d.secrets)
        return { state: 'committed', revision: 2 }
      }
      return { ...instance(s, 3), enabled: true, state: 'running' }
    },
    s,
    d,
    () => {},
  )
  assert.deepEqual(calls, ['create', 'configure', 'enable'])
})

test('a lost settings response retries the same operation without creating another instance', async () => {
  const s = session(),
    d = draft(),
    calls: { action: string; params: any }[] = []
  let fail = true
  const control = async (action: string, params: Record<string, unknown>) => {
    calls.push({ action, params })
    if (action === 'create') return instance(s)
    if (fail) {
      fail = false
      throw new Error('synthetic connection lost')
    }
    return { state: 'committed', revision: 2 }
  }
  await assert.rejects(submitPluginSetup(control, s, d, () => {}))
  await submitPluginSetup(control, s, d, () => {})
  assert.deepEqual(
    calls.map((c) => c.action),
    ['create', 'configure', 'configure'],
  )
  assert.deepEqual(calls[1].params.update, calls[2].params.update)
})

test('an edited retry recovers the prior receipt and uses its revision with a new operation', async () => {
  const s = session(),
    d = draft(),
    calls: { action: string; params: any }[] = []
  let fail = true
  const control = async (action: string, params: Record<string, unknown>) => {
    calls.push({ action, params })
    if (action === 'create') return instance(s)
    if (action === 'operation') return { state: 'committed', revision: 2 }
    if (fail) {
      fail = false
      throw new Error('synthetic response lost after commit')
    }
    return { state: 'committed', revision: 3 }
  }
  await assert.rejects(submitPluginSetup(control, s, d, () => {}))
  await submitPluginSetup(control, s, { ...d, config: { host: 'smtp.other.test' } }, () => {})
  assert.deepEqual(
    calls.map((c) => c.action),
    ['create', 'configure', 'operation', 'configure'],
  )
  assert.notEqual(calls[1].params.update.operation_id, calls[3].params.update.operation_id)
  assert.equal(calls[3].params.update.expected_revision, 2)
})

test('failed activation retains saved settings and confirms deactivation before a retry', async () => {
  const s = session(),
    d = { ...draft(), enable: true },
    calls: string[] = []
  let fail = true
  const control = async (action: string) => {
    calls.push(action)
    if (action === 'create') return instance(s)
    if (action === 'configure') return { state: 'committed', revision: 2 }
    if (action === 'enable' && fail) {
      fail = false
      throw new Error('synthetic startup failure')
    }
    return { ...instance(s, 4), enabled: action === 'enable' }
  }
  await assert.rejects(submitPluginSetup(control, s, d, () => {}))
  assert.equal(s.instance?.enabled, false)
  await submitPluginSetup(control, s, d, () => {})
  assert.deepEqual(calls, ['create', 'configure', 'enable', 'disable', 'enable'])
})

test('an uncertain creation keeps its identity and cannot silently switch resource or grants', async () => {
  const s = session(),
    d = draft(),
    calls: any[] = []
  let fail = true
  const control = async (action: string, params: Record<string, unknown>) => {
    calls.push({ action, params })
    if (fail) {
      fail = false
      throw new Error('synthetic creation response lost')
    }
    return action === 'create' ? instance(s) : { state: 'committed', revision: 2 }
  }
  await assert.rejects(submitPluginSetup(control, s, d, () => {}))
  await assert.rejects(
    submitPluginSetup(control, s, { ...d, grants: [] }, () => {}),
    (e: unknown) => e instanceof PluginSetupError && e.code === 'selection_locked',
  )
  await submitPluginSetup(control, s, d, () => {})
  assert.deepEqual(
    calls.slice(0, 2).map((c) => c.params),
    [s.creation, s.creation],
  )
})

test('uncertain activation is observed before retry and never reported as disabled', async () => {
  const s = session(),
    d = { ...draft(), enable: true },
    calls: string[] = []
  const control = async (action: string) => {
    calls.push(action)
    if (action === 'create') return instance(s)
    if (action === 'configure') return { state: 'committed', revision: 2 }
    if (action === 'catalog')
      return { instances: [{ ...instance(s, 3), enabled: true, state: 'running' }] }
    throw new Error('synthetic activation or cleanup response lost')
  }
  await assert.rejects(
    submitPluginSetup(control, s, d, () => {}),
    (error: unknown) => error instanceof PluginSetupError && error.code === 'activation_pending',
  )
  assert.equal(s.activationAttempted, true)
  const result = await submitPluginSetup(control, s, d, () => {})
  assert.equal(result.enabled, true)
  assert.equal(result.state, 'running')
  assert.deepEqual(calls, ['create', 'configure', 'enable', 'disable', 'catalog'])
})
