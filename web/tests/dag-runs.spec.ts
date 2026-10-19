import { test, expect, type Page } from '@playwright/test'
const workspace = '11111111-1111-4111-8111-111111111111'
const id = '00000000-0000-4000-8000-000000000001'
const child = '00000000-0000-4000-8000-000000000002'
const at = '2026-10-20T10:00:00Z'
function fixture() {
  const node = {
    id: 'collect',
    name: 'Collect sources',
    pre: [],
    runner: 'Preview device',
    agent: 'pi',
    cwd: '/workspace',
    prompt: 'Collect sources',
    parameters: {},
    timeout: '1h',
  }
  const config = {
    version: 1,
    id: 'preview-dag',
    name: 'Daily research',
    runner: '',
    agent: '',
    cwd: '',
    prompt: '',
    parameters: {},
    schedule: null,
    enabled: true,
    execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
    dag: { nodes: [node] },
  }
  const run: any = {
    id,
    task_id: config.id,
    state: 'running',
    reason: null,
    result: null,
    task_archived: false,
    config,
    source: 'manual',
    created_at: at,
    scheduled_at: at,
    started_at: at,
    finished_at: null,
    nodes: [{ ...node, state: 'running', execution_id: child, continuation_available: true }],
  }
  const execution = {
    ...run,
    id: child,
    parent_run_id: id,
    node_id: node.id,
    nodes: undefined,
    config: { ...config, ...node, dag: undefined },
    execution_snapshot: { trace_supported: true, cwd: '/workspace', parameters: {} },
    session: {
      id: 'native-session',
      runner_id: 'preview-device',
      agent: 'pi',
      cwd: '/workspace',
      ready: true,
      command: 'pi --resume native-session',
    },
  }
  const events = [
    {
      id: 1,
      run_id: child,
      seq: 1,
      kind: 'trace',
      created_at: at,
      data: {
        id: 'tool',
        kind: 'tool',
        status: 'succeeded',
        title: 'Read sources',
        input: { file: 'sources.md' },
        output: 'Primary sources',
        occurred_at: at,
        started_at: at,
        finished_at: '2026-10-20T10:00:01Z',
      },
    },
  ]
  return { run, execution, events }
}
async function mock(page: Page, f: ReturnType<typeof fixture>) {
  const writes: string[] = []
  await page.route('**/api/v1/**', async (route) => {
    const url = new URL(route.request().url()),
      path = url.pathname
    if (route.request().method() !== 'GET') writes.push(path)
    let value: any = []
    if (path === '/api/v1/session')
      value = { id: 'preview-user', username: 'preview', role: 'admin' }
    else if (path === '/api/v1/meta') value = { version: '0.2.0' }
    else if (path === `/api/v1/workspaces/${workspace}`)
      value = { id: workspace, name: 'Preview', role: 'admin' }
    else if (path.endsWith('/workspaces'))
      value = { items: [{ id: workspace, name: 'Preview', role: 'admin' }], demo: null }
    else if (path.endsWith('/settings')) value = { timezone: 'UTC' }
    else if (path.endsWith('/runs')) value = { items: [f.run], total: 1, page: 1, page_size: 20 }
    else if (path.endsWith(`/runs/${id}`)) value = f.run
    else if (path.endsWith('/nodes/collect'))
      value = { node: f.run.nodes[0], execution: f.execution }
    else if (path.endsWith('/events'))
      value = f.events.filter((e) => e.id > Number(url.searchParams.get('after') ?? 0))
    else if (path.endsWith('/stream'))
      return route.fulfill({ contentType: 'text/event-stream', body: ': fixture\n\n' })
    else if (path.endsWith('/agents'))
      value = [
        {
          id: 'pi',
          name: 'Pi',
          runner: 'Preview device',
          runner_id: 'preview-device',
          logo: '/agent-logos/pi.svg',
          available: true,
        },
      ]
    else if (path.endsWith('/runners'))
      value = [{ id: 'preview-device', name: 'Preview device', agents: [] }]
    else if (path.endsWith('/moments/days'))
      value = {
        timezone: 'UTC',
        today: '2026-10-20',
        total_runs: 1,
        days: [{ date: '2026-10-20', count: 1 }],
        next_before: null,
      }
    else if (path.endsWith('/moments'))
      value = {
        timezone: 'UTC',
        date: '2026-10-20',
        items: [
          {
            id,
            task_id: 'preview-dag',
            name: 'Daily research',
            state: 'warning',
            reason: 'Independent branch failed',
            finished_at: at,
            format: 'dag',
            node_count: 2,
            continuation_available: true,
            preview: null,
            outputs: [
              {
                node_id: 'collect',
                name: 'Collect sources',
                state: 'succeeded',
                preview: '# Preview report',
                format: 'markdown',
                has_more: true,
                truncated: false,
              },
            ],
            has_more: true,
            truncated: false,
          },
        ],
        next_cursor: null,
      }
    return route.fulfill({ json: value })
  })
  return writes
}
function finish(f: ReturnType<typeof fixture>) {
  f.run.state = 'warning'
  f.run.reason = 'Independent branch failed'
  f.run.finished_at = at
  f.run.result = {
    type: 'dag',
    outputs: [
      {
        node_id: 'collect',
        name: 'Collect sources',
        state: 'succeeded',
        result: '# Final report\n\n| Source | Status |\n|---|---|\n| Primary | Ready |',
      },
    ],
  }
  f.run.nodes[0].state = 'succeeded'
  f.execution.state = 'succeeded'
}
test('DAG highlights output once, preserves trace expansion and uses only whole-graph actions', async ({
  page,
}) => {
  const f = fixture(),
    writes = await mock(page, f)
  await page.goto(`/w/${workspace}/runs?run=${id}`)
  await expect(page.locator('.dag-graph-node')).toHaveCount(1)
  await page.getByText('Read sources', { exact: true }).click()
  await expect(page.getByText('Primary sources', { exact: true })).toBeVisible()
  finish(f)
  await expect(page.getByRole('heading', { name: 'Final report', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '过程', exact: true })).toHaveAttribute(
    'aria-expanded',
    'false',
  )
  await page.getByRole('button', { name: '过程', exact: true }).click()
  await expect(page.getByText('Primary sources', { exact: true })).toBeVisible()
  await page.waitForTimeout(2300)
  await expect(page.getByRole('button', { name: '过程', exact: true })).toHaveAttribute(
    'aria-expanded',
    'true',
  )
  await page.getByRole('button', { name: '原文', exact: true }).click()
  await expect(page.locator('.result-raw')).toContainText('"type": "dag"')
  await page.getByRole('button', { name: '阅读', exact: true }).click()
  await expect(page.locator('.trace-notice')).toHaveCount(0)
  await page.locator('.dag-run-page').evaluate((el) => (el.scrollTop = 0))
  for (const [width, theme] of [
    [1440, 'light'],
    [390, 'dark'],
    [2200, 'light'],
  ] as const) {
    await page.setViewportSize({ width, height: 900 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({ path: `test-results/dag-run-${width}-${theme}.png`, fullPage: true })
  }
  expect(writes).toEqual([])
})
test('DAG completion does not leave details; Moments previews reports and resumes the selected actual node', async ({
  page,
}) => {
  const f = fixture()
  await mock(page, f)
  await page.goto(`/w/${workspace}/runs?run=${id}`)
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  finish(f)
  await expect(page.getByRole('tab', { name: '详情', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await page.getByRole('tab', { name: '执行', exact: true }).click()
  await expect(page.getByRole('heading', { name: 'Final report', exact: true })).toBeVisible()
  await page.goto(`/w/${workspace}/`)
  await expect(page.getByRole('heading', { name: 'Preview report', exact: true })).toBeVisible()
  await expect(page.locator('.moment-identity')).toContainText('2')
  await expect(page.locator('.moment-identity')).not.toContainText('Preview device')
  await page.getByRole('button', { name: '阅读全文', exact: true }).click()
  await expect(page.getByRole('heading', { name: 'Final report', exact: true })).toBeVisible()
  await page.getByRole('button', { name: '续聊', exact: true }).click()
  await expect(page.getByRole('dialog')).toContainText('pi --resume native-session')
  await expect(page.getByRole('dialog')).toContainText('/workspace')
})
