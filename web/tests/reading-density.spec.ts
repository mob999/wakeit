import { test, expect, type Page } from '@playwright/test'
import { mockWorkspace, workspacePage, apiPath } from './workspace'
const id = '00000000-0000-4000-8000-000000000001',
  at = '2026-10-20T01:00:00Z'
async function fixture(page: Page, dag = 0, count = 100) {
  const base = {
    name: 'Synthetic research',
    pre: [],
    runner: 'Synthetic device',
    agent: 'pi',
    cwd: '/workspace',
    prompt: 'Synthetic instruction',
    parameters: {},
  }
  const nodes = Array.from({ length: dag }, (_, i) => ({
    ...base,
    id: 'node-' + i,
    name: `Synthetic node ${i + 1} — 多语言节点名称`,
    execution_id: `00000000-0000-4000-8000-${String(i + 2).padStart(12, '0')}`,
    state: 'running',
  }))
  const config: any = {
    ...base,
    version: 1,
    id: 'synthetic',
    name: 'Synthetic reading fixture',
    enabled: true,
    schedule: null,
    execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
  }
  if (dag) config.dag = { nodes: nodes.map(({ execution_id, state, ...node }) => node) }
  const run: any = {
    id,
    task_id: 'synthetic',
    config,
    state: 'running',
    reason: null,
    result: null,
    created_at: at,
    started_at: at,
    finished_at: null,
    source: 'manual',
    execution_snapshot: { trace_supported: true, cwd: '/workspace' },
    ...(dag ? { nodes } : {}),
  }
  const rows: any[] = Array.from({ length: count }, (_, i) => ({
    id: i + 1,
    seq: i + 1,
    run_id: id,
    created_at: at,
    kind: 'trace',
    data: {
      id: 'message-' + i,
      title: '消息',
      kind: 'message',
      status: 'succeeded',
      input: null,
      output: i === 0 ? '' : `# Message ${i}\n\nSynthetic message body ${i}`,
      occurred_at: at,
      started_at: at,
      finished_at: '2026-10-20T01:00:01Z',
    },
  }))
  rows.push({
    id: count + 1,
    seq: count + 1,
    run_id: id,
    created_at: at,
    kind: 'trace',
    data: {
      id: 'tool',
      title: 'read',
      kind: 'tool',
      status: 'succeeded',
      parent_id: 'message-1',
      input: { path: '/workspace/synthetic.md' },
      output: Array.from({ length: 30 }, (_, i) => `output line ${i + 1}`).join('\n'),
      occurred_at: at,
      started_at: at,
      finished_at: '2026-10-20T01:00:02Z',
    },
  })
  const requests: number[] = [],
    writes: string[] = []
  await page.route('**/api/v1/**', async (route) => {
    if (await mockWorkspace(route)) return
    const url = new URL(route.request().url()),
      path = apiPath(url.pathname)
    if (route.request().method() !== 'GET') writes.push(path)
    let value: any = []
    if (path === '/session') value = { id: 'synthetic-user', username: 'preview', role: 'admin' }
    else if (path === '/meta') value = { version: '0.3.0' }
    else if (path === '/settings') value = { timezone: 'UTC' }
    else if (path === '/runners')
      value = [{ id: 'device', name: 'Synthetic device', online: true, agents: [] }]
    else if (path === '/runs') value = { items: [run], total: 1, page: 1, page_size: 20 }
    else if (path === `/runs/${id}`) value = run
    else if (path.includes('/nodes/')) {
      const node = nodes.find((n) => path.endsWith('/' + n.id)) ?? nodes[0]
      value = {
        node,
        execution: {
          ...run,
          id: node.execution_id,
          parent_run_id: id,
          node_id: node.id,
          nodes: undefined,
          config: { ...config, ...base, dag: undefined },
          state: node.state,
          result: null,
          reason: (node as any).reason ?? null,
        },
      }
    } else if (path.endsWith('/events')) {
      const after = Number(url.searchParams.get('after') ?? 0)
      requests.push(after)
      value = rows.filter((r) => r.id > after).slice(0, 500)
    } else if (path.endsWith('/stream'))
      return route.fulfill({
        contentType: 'text/event-stream',
        body:
          'retry: 1000\n\n' +
          rows.map((r) => `event: run_event\ndata: ${JSON.stringify(r)}\n\n`).join(''),
      })
    return route.fulfill({ json: value })
  })
  await page.goto(workspacePage(`/runs?run=${id}`))
  return { run, rows, requests, writes }
}
test('compact history, empty messages, independent payload folds and full copy', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  const f = await fixture(page),
    trace = page.locator('.run-trace')
  await expect(trace.locator('.trace-row')).toHaveCount(101)
  await expect(trace.locator('details[open]')).toHaveCount(1)
  await expect(trace.locator('[data-trace-id="message-0"] .trace-message')).toHaveCount(0)
  expect(
    await trace
      .locator('[data-trace-id="message-1"]')
      .evaluate((e) => e.getBoundingClientRect().height),
  ).toBe(36)
  await page.getByRole('button', { name: '全部收起', exact: true }).click()
  await expect(trace.locator('details[open]')).toHaveCount(0)
  const tool = trace.locator('[data-trace-id="tool"]')
  await tool.locator('summary').click()
  await expect(tool.locator('.trace-payload-section')).toHaveCount(2)
  const output = tool.locator('.trace-payload-section').last()
  await expect(output.locator('pre')).toContainText('output line 12')
  await expect(output.locator('pre')).not.toContainText('output line 13')
  await output.getByRole('button', { name: '展开全文', exact: true }).click()
  await expect(output.locator('pre')).toContainText('output line 30')
  await output.getByRole('button', { name: '复制', exact: true }).click()
  await expect
    .poll(() => page.evaluate(() => navigator.clipboard.readText()))
    .toBe(f.rows.at(-1).data.output)
  await output.getByRole('button', { name: '输出', exact: true }).click()
  await expect(output.locator('pre')).toHaveCount(0)
  await tool.locator('summary').press('Enter')
  await expect(tool).not.toHaveAttribute('open')
  await page.getByRole('button', { name: '回到最新', exact: true }).click()
  await expect(trace.locator('[data-trace-id="message-99"]')).toHaveAttribute('open')
  expect(f.writes).toEqual([])
})
test('new messages and ready results do not interrupt reading; tabs preserve the anchor', async ({
  page,
}) => {
  const f = await fixture(page),
    trace = page.locator('.run-trace'),
    pane = page.locator('.run-execution-page')
  await expect(trace.locator('.trace-row')).toHaveCount(101)
  await pane.evaluate((el) => (el.scrollTop = 0))
  await expect(trace).toHaveAttribute('data-follow', 'false')
  const before = await pane.evaluate((el) => el.scrollTop)
  f.rows.push({
    ...f.rows[1],
    id: 200,
    seq: 200,
    data: { ...f.rows[1].data, id: 'new-message', output: '# New message' },
  })
  await expect(trace.locator('[data-trace-id="new-message"]')).toBeAttached()
  await expect(page.getByRole('button', { name: '1 条更新', exact: true })).toBeVisible()
  await expect(trace.locator('[data-trace-id="message-99"]')).toHaveAttribute('open')
  await expect(trace.locator('[data-trace-id="new-message"]')).not.toHaveAttribute('open')
  expect(Math.abs((await pane.evaluate((el) => el.scrollTop)) - before)).toBeLessThan(2)
  f.run.state = 'succeeded'
  f.run.result = '# Final report'
  f.run.finished_at = '2026-10-20T01:01:00Z'
  const ready = page.getByRole('button', { name: '结果已就绪', exact: true })
  await expect(ready).toBeInViewport()
  await expect(trace).toBeVisible()
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await page.getByRole('tab', { name: '执行', exact: true }).click()
  await expect(trace).toHaveAttribute('data-follow', 'false')
  expect(Math.abs((await pane.evaluate((el) => el.scrollTop)) - before)).toBeLessThan(2)
  await ready.click()
  await expect(page.getByRole('heading', { name: 'Final report', exact: true })).toBeVisible()
  await expect(trace).toBeHidden()
  expect(f.writes).toEqual([])
})
test('DAG table is bounded and selected process is accessible; structure collapses separately', async ({
  page,
}) => {
  const f = await fixture(page, 62, 4)
  await page.getByRole('button', { name: '表格', exact: true }).click()
  const table = page.locator('.dag-run-structure .dag-node-table')
  await expect(table.getByRole('row')).toHaveCount(63)
  expect(await table.evaluate((el) => el.getBoundingClientRect().height)).toBeLessThanOrEqual(280)
  await table.getByRole('row', { name: f.run.nodes[61].name, exact: true }).click()
  const context = page.locator('.dag-node-process:not([hidden]) > .dag-node-context')
  await expect(context).toContainText(f.run.nodes[61].name)
  await expect(context).toBeInViewport()
  await page.getByRole('button', { name: '收起结构', exact: true }).click()
  await expect(table).toBeHidden()
  await expect(page.locator('.dag-node-process:not([hidden]) .run-trace')).toBeVisible()
  await page.getByRole('button', { name: '展开结构', exact: true }).click()
  await expect(table).toBeVisible()
  expect(f.writes).toEqual([])
})
for (const [width, language, theme, nodes] of [
  [390, 'zh', 'light', 1],
  [1440, 'en', 'light', 20],
  [1440, 'zh', 'dark', 100],
  [1920, 'en', 'dark', 62],
] as const)
  test(`synthetic reading layout ${width}px ${language} ${theme} ${nodes} nodes`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 })
    await page.addInitScript(
      (language) => localStorage.setItem('wakeit-language', language),
      language,
    )
    await fixture(page, nodes, 6)
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    const trace = page.locator('.dag-node-process:not([hidden]) .run-trace')
    await expect(trace.locator('.trace-row')).toHaveCount(7)
    await expect(page.locator('.dag-run-structure .dag-graph-node')).toHaveCount(nodes)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await expect(
      page.locator('.dag-node-process:not([hidden]) > .dag-node-context'),
    ).toBeInViewport()
    const close = page.locator('.toast .icon-button')
    if (await close.isVisible()) await close.click()
    await page.screenshot({ path: `test-results/reading-${width}-${language}-${theme}.png` })
  })
test('paged history and duplicated/late terminal snapshots retain order and real status', async ({
  page,
}) => {
  const f = await fixture(page, 0, 550)
  await expect(page.locator('.trace-row')).toHaveCount(551)
  expect(f.requests).toContain(500)
  f.rows.push({
    ...f.rows[1],
    id: 900,
    seq: 900,
    data: {
      ...f.rows[1].data,
      id: 'tool',
      kind: 'tool',
      title: 'read',
      status: 'failed',
      output: 'Actual tool failure',
    },
  })
  await expect(page.locator('[data-trace-id="tool"] .trace-state')).toHaveText('失败')
  f.rows.push({
    ...f.rows.at(-1),
    id: 901,
    seq: 800,
    data: { ...f.rows.at(-1).data, status: 'running', output: 'Late running snapshot' },
  })
  await page.waitForTimeout(1200)
  await expect(page.locator('[data-trace-id="tool"] .trace-state')).toHaveText('失败')
  await expect(page.locator('.trace-row')).toHaveCount(551)
})

test('failure reasons remain persistent context while successful tools retain their actual status', async ({
  page,
}) => {
  const f = await fixture(page, 1, 4)
  f.run.nodes[0].state = 'failed'
  f.run.nodes[0].reason = 'Synthetic child failure'
  f.run.reason = 'Synthetic graph failure'
  f.run.state = 'failed'
  await expect(page.locator('.dag-outcome .run-reason')).toContainText('Synthetic graph failure')
  await expect(page.locator('.dag-node-process:not([hidden]) .trace-state.succeeded')).toHaveCount(
    5,
  )
  await expect(page.locator('.dag-node-process:not([hidden]) .run-reason')).toContainText(
    'Synthetic child failure',
  )
  await expect(page.locator('.dag-node-reason')).toHaveCount(0)
  await page.locator('.dag-outcome .run-reason').click()
  await expect(page.locator('.dag-outcome .run-reason-full')).toHaveText('Synthetic graph failure')
})
