import { t as tr } from '../i18n'
import { apiErrorText } from './errorText'
import { stateNames } from '../i18n'

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
    public body?: unknown,
  ) {
    super(message)
  }
}

// A document belongs to one workspace. Switching loads a new document, so delayed
// mutations and queries retain the scope in which they were initiated.
export const workspaceId = /^\/w\/([0-9a-f-]{36})(?:\/|$)/i.exec(location.pathname)?.[1]

export const workspaceBase = workspaceId ? `/w/${workspaceId}` : ''

export function apiUrl(path: string) {
  const global =
    /^\/(session(?:$|\?)|meta(?:$|\?)|changelog(?:$|\?)|workspaces(?:$|[/?])|instance\/|oauth\/|auth\/|device-authorizations\/|invites\/(accept|inspect)|register(?:$|\?)|schema\/|openapi\.json)/.test(
      path,
    )
  if (!global && !workspaceId) throw new Error(tr('请选择工作区'))
  return `/api/v1${global ? '' : `/workspaces/${workspaceId}`}${path}`
}

export async function api<T = any>(
  path: string,
  method = 'GET',
  body?: unknown,
  key?: string,
  signal?: AbortSignal,
): Promise<T> {
  const response = await fetch(apiUrl(path), {
    method,
    signal,
    credentials: 'same-origin',
    headers: {
      'Content-Type': 'application/json',
      'X-Wakeit-Request': '1',
      ...(method === 'POST' ? { 'Idempotency-Key': key ?? crypto.randomUUID() } : {}),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const value = await response.json()
  if (response.status === 401 && path !== '/session')
    window.dispatchEvent(new Event('wakeit:unauthorized'))
  if (!response.ok)
    throw new ApiError(
      response.status,
      typeof value.error === 'string'
        ? apiErrorText(value.error, tr, (state) => stateNames[state] ?? state)
        : tr('请求失败 ({{v0}})', { v0: response.status }),
      value,
    )
  return value
}
