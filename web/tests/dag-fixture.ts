import { expect, type Page } from '@playwright/test'
export const workspace = '11111111-1111-4111-8111-111111111111'
export const runnerId = '22222222-2222-4222-8222-222222222222'
export const base = '/w/' + workspace + '/tasks/dag'
export function definition() {
  const node = {
    id: 'source',
    name: 'Collect sources',
    pre: [] as string[],
    runner: runnerId,
    agent: 'pi',
    cwd: '/workspace',
    prompt: 'Find primary sources',
    parameters: { model: 'model-a' },
    session: null as { id: string } | null,
    timeout: '1h',
  }
  return {
    version: 1,
    id: 'ux-fixture',
    name: 'Daily report',
    enabled: false,
    schedule: { cron: '0 9 * * *', timezone: 'Asia/Shanghai' } as any,
    execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
    dag: {
      nodes: [
        node,
        {
          ...structuredClone(node),
          id: 'report',
          name: 'Write report',
          pre: ['source'],
          prompt: '{{ nodes.source.result }}',
        },
      ],
    },
  }
}
export async function fixture(page: Page, config = definition()) {
  let layout: any = null
  const requests: { path: string; method: string; body: any }[] = []
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname.replace(
      /^\/api\/v1(?:\/workspaces\/[0-9a-f-]{36})?/,
      '',
    )
    const method = route.request().method(),
      body = route.request().postDataJSON()
    requests.push({ path, method, body })
    let json: any = []
    if (path === '/session') json = { id: 'preview-user', username: 'preview', role: 'admin' }
    else if (path === '/meta') json = { version: '0.2.0' }
    else if (path === '/workspaces')
      json = { items: [{ id: workspace, name: 'Preview', role: 'admin' }] }
    else if (path === '/' + workspace) json = { id: workspace, name: 'Preview', role: 'admin' }
    else if (new URL(route.request().url()).pathname === '/api/v1/workspaces/' + workspace)
      json = { id: workspace, name: 'Preview', role: 'admin' }
    else if (path === '/settings') json = { timezone: 'Asia/Shanghai' }
    else if (path === '/runners')
      json = [
        {
          id: runnerId,
          name: 'Preview device',
          online: true,
          revoked: false,
          directory_roots: ['/workspace'],
          agents: [
            {
              id: 'pi',
              name: 'Pi',
              logo: '/agent-logos/pi.svg',
              available: true,
              mode: 'sync',
              digest: 'fixture',
              session_supported: true,
              session_list_supported: true,
              models_supported: true,
              parameters: {
                model: { type: 'string', label: 'Model' },
                reasoning: { type: 'string', label: 'Reasoning', enum: ['low', 'high'] },
              },
            },
          ],
        },
      ]
    else if (path === '/tasks/ux-fixture')
      json =
        method === 'PUT'
          ? { id: config.id, revision: 8, config: body.config, editor_layout: body.editor_layout }
          : { id: config.id, revision: 7, config, editor_layout: layout, archived_at: null }
    else if (path.endsWith('/models/query'))
      json = {
        version: 1,
        models: [
          { id: 'a', name: 'Model A', parameters: { model: 'model-a' }, parameter_options: {} },
          { id: 'b', name: 'Model B', parameters: { model: 'model-b' }, parameter_options: {} },
        ],
      }
    else if (path.endsWith('/sessions/query'))
      json = { version: 1, sessions: [], next_cursor: null }
    else if (path.endsWith('/sessions/inspect'))
      json = { id: body.id, title: 'Preview session', cwd: '/workspace', persisted: true }
    else if (path.endsWith('/directories'))
      json = {
        path: '/workspace',
        parent: null,
        breadcrumbs: [{ name: 'workspace', path: '/workspace' }],
        directories: [],
        next_cursor: null,
      }
    else if (path === '/timezones') json = ['Asia/Shanghai', 'UTC', 'America/New_York']
    else if (path === '/schedule/preview') json = ['2026-10-20T01:00:00Z']
    else if (path === '/tasks/validate') json = { errors: [] }
    return route.fulfill({ json })
  })
  return {
    config,
    requests,
    setLayout: (value: any) => {
      layout = value
    },
  }
}
export async function editNode(page: Page, id = 'source') {
  await expect(page.locator('.dag-interactive-canvas')).toHaveAttribute('data-layout-ready', 'true')
  await page
    .locator('.react-flow__node')
    .filter({ hasText: id === 'source' ? 'Collect sources' : 'Write report' })
    .click()
}
export async function switchNode(page: Page, label: string) {
  await page
    .locator('.dag-floating-header')
    .getByRole('combobox', { name: '选择节点', exact: true })
    .click()
  await page.getByRole('option', { name: new RegExp('^' + label) }).click()
}
