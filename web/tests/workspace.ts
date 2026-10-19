import type { Route } from '@playwright/test'

export const fixtureWorkspace = '11111111-2222-4333-8444-555555555555'
export function workspacePage(path: string) {
  return path === '/' || /^\/(tasks|runs|runners|settings)(?:[/?]|$)/.test(path)
    ? `/w/${fixtureWorkspace}${path}` : path
}
export function apiPath(path: string) {
  return path.replace(/^\/api\/v1(?:\/workspaces\/[0-9a-f-]{36}(?=\/|$))?/, '')
}
export async function mockWorkspace(route: Route, role = 'admin') {
  const path = new URL(route.request().url()).pathname
  const workspace = { id: fixtureWorkspace, name: 'Default', role, is_default: true, is_demo: false }
  if (path === '/api/v1/workspaces') {
    await route.fulfill({ json: { items: [workspace], demo: null } })
    return true
  }
  if (path === `/api/v1/workspaces/${fixtureWorkspace}`) {
    await route.fulfill({ json: workspace })
    return true
  }
  return false
}
