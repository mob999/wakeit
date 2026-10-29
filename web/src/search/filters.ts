import { type Runner, type Agent } from '../api/types'

export type Scope = 'presets' | 'tasks' | 'runs' | 'runners' | 'agents' | 'settings'

export type Filters = {
  q?: string
  state?: string
  runner?: string
  agent?: string
  enabled?: string
  online?: string
  available?: string
  mode?: string
}

export function listPath(
  kind: 'tasks' | 'runs',
  filters: Filters,
  page: number,
  extra: Record<string, string> = {},
) {
  const params = new URLSearchParams({ page: String(page), page_size: '20', ...extra })
  for (const key of ['q', 'state', 'runner', 'agent', 'enabled'] as const)
    if (filters[key]) params.set(key, filters[key]!)
  return `/${kind}?${params}`
}

export function matches(text: string, q = '') {
  const haystack = text.toLocaleLowerCase()
  return q
    .trim()
    .toLocaleLowerCase()
    .split(/\s+/)
    .every((term) => haystack.includes(term))
}

export const runnerMatches = (r: Runner, f: Filters) =>
  matches(`${r.name} ${r.id} ${r.info.os ?? ''} ${r.agents.map((a) => a.name).join(' ')}`, f.q) &&
  (!f.online ||
    (f.online === 'revoked' ? r.revoked : !r.revoked && r.online === (f.online === 'online')))

export const agentMatches = (a: Agent, f: Filters) =>
  matches(`${a.name} ${a.id} ${a.runner ?? ''}`, f.q) &&
  (!f.runner || a.runner === f.runner) &&
  (!f.available || a.available === (f.available === 'true')) &&
  (!f.mode || a.mode === f.mode)
