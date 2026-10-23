import { languageAction } from './actions'
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
test('recording causes stay in the toolbar tooltip, with history and diagnostics kept intact', async ({
  page,
}) => {
  const f = await fixture(page, 0, 2)
  f.rows.push(
    ...[
      ['truncated', 'content_shortened'],
      ['truncated', 'content_shortened'],
      ['incomplete', 'event_parse_failed'],
      ['expired', undefined],
      ['incomplete', undefined],
    ].map(([status, reason], index) => ({
      id: 100 + index,
      seq: 100 + index,
      kind: 'trace_status',
      created_at: at,
      data: { status, reason },
    })),
    {
      id: 105,
      seq: 105,
      kind: 'log',
      created_at: at,
      data: { text: 'Ordinary adapter diagnostic', diagnostic: true },
    },
  )
  await page.reload()
  const info = page.getByRole('button', { name: '过程记录提示', exact: true })
  await expect(info).toHaveCount(1)
  await expect(page.locator('.run-trace .trace-row')).toHaveCount(3)
  await expect(page.locator('.run-trace .trace-retention')).toHaveCount(0)
  await expect(page.getByText('部分事件未记录', { exact: true })).toHaveCount(0)
  await info.focus()
  const tip = page.getByRole('tooltip')
  await expect(tip).toContainText('仅保存了节选')
  await expect(tip).toContainText('无法解析')
  await expect(tip).toContainText('保留期限清理')
  await expect(tip).toContainText('未提供具体原因')
  expect((await tip.innerText()).match(/仅保存了节选/g)).toHaveLength(1)
  await page.keyboard.press('Escape')
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await expect(page.locator('.logs')).toContainText('Ordinary adapter diagnostic')
  await page.getByRole('tab', { name: '轨迹', exact: true }).click()
  await (await languageAction(page, '切换到英文')).click()
  const english = page.getByRole('button', { name: 'Activity recording info', exact: true })
  await english.focus()
  await expect(tip).toContainText('saved as excerpts')
  await expect(tip).toContainText('could not be parsed')
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 })
    await expect(english).toBeVisible()
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  }
  // Diagnostic logs alone must not recreate the old incomplete banner or toolbar notice.
  f.rows.splice(3, 5)
  await page.reload()
  await expect(
    page.getByRole('button', { name: 'Activity recording info', exact: true }),
  ).toHaveCount(0)
  await expect(page.locator('.run-trace .trace-row')).toHaveCount(3)
  expect(f.writes).toEqual([])
})

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
  async function expectAlignedIcons() {
    const headings = await trace
      .locator('.trace-operation > summary, .trace-heading')
      .evaluateAll((elements) =>
        elements.map((element) => {
          const label = element.querySelector('.trace-label')!.getBoundingClientRect()
          return [...element.querySelectorAll(':scope > svg')].map((icon) => {
            const rect = icon.getBoundingClientRect()
            return {
              x: rect.x,
              offset: Math.abs(rect.y + rect.height / 2 - label.y - label.height / 2),
            }
          })
        }),
      )
    expect(headings).toHaveLength(101)
    for (const icons of headings) {
      for (const icon of icons) expect(icon.offset).toBeLessThan(0.5)
      expect(icons[1].x).toBeCloseTo(headings[0][1].x, 1)
    }
  }
  await expectAlignedIcons()
  await page.setViewportSize({ width: 390, height: 844 })
  await expectAlignedIcons()
  await page.setViewportSize({ width: 1440, height: 900 })
  expect(
    await trace
      .locator('[data-trace-id="message-1"]')
      .evaluate((e) => e.getBoundingClientRect().height),
  ).toBe(36)
  await page.getByRole('button', { name: '全部收起', exact: true }).click()
  await expect(trace.locator('details[open]')).toHaveCount(0)
  const tool = trace.locator('[data-trace-id="tool"]')
  await tool.locator('summary').click()
  await expectAlignedIcons()
  await expect(tool.locator('.trace-payload-section')).toHaveCount(2)
  const input = tool.locator('.trace-payload-section').first()
  await expect(input.locator('code')).toHaveClass(/language-json/)
  await expect(input.locator('.hljs-attr')).toHaveText('"path"')
  await expect(input.locator('.hljs-string')).toHaveText('"/workspace/synthetic.md"')
  await expect(input.locator('pre')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)')
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 })
    for (const section of await tool.locator('.trace-payload-section').all()) {
      const label = section.locator('.trace-payload-heading > button')
      await expect(section).toHaveCSS('border-top-width', '0px')
      await expect(label).toHaveCSS('font-size', '12px')
      await expect(section.locator('pre')).toHaveCSS('font-size', '12px')
      await expect(section.locator('pre')).toHaveCSS('line-height', '20px')
      await expect
        .poll(() =>
          section.evaluate((el) => {
            const toggle = el.querySelector('.trace-payload-heading > button')!
            const icon = toggle.querySelector('svg')!.getBoundingClientRect()
            const code = el.querySelector('pre')!.getBoundingClientRect()
            return Math.abs(code.x - icon.right - parseFloat(getComputedStyle(toggle).gap))
          }),
        )
        .toBeLessThan(0.5)
      await expect(section.getByRole('button', { name: '复制', exact: true })).toHaveCSS(
        'width',
        '28px',
      )
    }
  }
  await page.setViewportSize({ width: 1440, height: 900 })
  await tool.screenshot({ path: 'test-results/trace-payload-transparent.png' })
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
test('result code and raw JSON stay transparent with syntax colors in both themes', async ({
  page,
}) => {
  const f = await fixture(page, 0, 4)
  f.run.state = 'succeeded'
  f.run.result = '# Report\n\n```json\n{"status": "ready", "count": 2}\n```\n\nInline `value`.'
  const result = page.locator('.run-result-panel')
  await expect(result.getByRole('heading', { name: 'Report', exact: true })).toBeVisible()
  for (const theme of ['light', 'dark']) {
    await page.evaluate((value) => (document.documentElement.dataset.theme = value), theme)
    await expect(result.locator('pre')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)')
    await expect(result.locator('code').last()).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)')
    const colors = await result
      .locator('pre')
      .evaluate((el) => [
        getComputedStyle(el.querySelector('.hljs-attr')!).color,
        getComputedStyle(el.querySelector('.hljs-string')!).color,
      ])
    expect(colors[0]).not.toBe(colors[1])
  }
  await page.getByRole('button', { name: '原文', exact: true }).click()
  await expect(result.locator('pre.result-raw')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)')
  f.run.result = { status: 'ready', count: 2, content: '```\n<script>literal text</script>' }
  await expect(result.locator('code.language-json')).toBeVisible()
  await expect(result.locator('pre')).toHaveText(JSON.stringify(f.run.result, null, 2) + '\n')
  await expect(result.locator('pre')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)')
  await expect(result.locator('.hljs-attr').first()).toHaveText('"status"')
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
  const ready = page.getByRole('button', { name: '输出已就绪', exact: true })
  await expect(ready).toBeInViewport()
  await expect(trace).toBeVisible()
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await page.getByRole('tab', { name: '轨迹', exact: true }).click()
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
  await page.addInitScript(() => {
    window.EventSource = class extends EventTarget {
      close() {}
    } as unknown as typeof EventSource
  })
  const f = await fixture(page, 1, 4)
  f.run.nodes[0].state = 'failed'
  f.run.nodes[0].reason = 'Synthetic child failure'
  f.run.reason = 'Synthetic graph failure'
  f.run.state = 'failed'
  await expect(page.locator('.dag-run-meta .run-reason')).toHaveAttribute(
    'aria-label',
    '运行状态：失败。原因：Synthetic graph failure',
  )
  await expect(page.locator('.dag-node-process:not([hidden]) .trace-state.succeeded')).toHaveCount(
    5,
  )
  const process = page.locator('.dag-node-process:not([hidden])')
  const childReason = process.locator('.run-reason')
  await expect(childReason).toHaveAttribute('aria-label', '失败原因: Synthetic child failure')
  await expect(childReason).toHaveText('失败')
  await expect(page.locator('.dag-node-reason')).toHaveCount(0)
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 })
    await expect
      .poll(() =>
        process.locator('.dag-node-context').evaluate((el) => {
          const context = el.getBoundingClientRect()
          const status = el.querySelector('.run-reason')!.getBoundingClientRect()
          return status.x - context.x - context.width / 2
        }),
      )
      .toBeGreaterThan(0)
    await childReason.hover()
    await expect(page.getByRole('tooltip')).toContainText('Synthetic child failure')
    await childReason.press('Escape')
    await expect(page.getByRole('tooltip')).toHaveCount(0)
  }
  await process.getByRole('button', { name: '全部收起', exact: true }).focus()
  await childReason.focus()
  await expect(page.getByRole('tooltip')).toContainText('Synthetic child failure')
  await childReason.press('Escape')
  await page.locator('.dag-run-meta .run-reason').hover()
  await expect(page.getByRole('tooltip')).toContainText('Synthetic graph failure')
  await expect(page.locator('.run-reason-full')).toHaveCount(0)
  await expect(page.locator('.dag-outcome')).toHaveCount(0)
})

test('run-level issues stay outside reports and prioritize current failures over skipped descendants', async ({
  page,
}) => {
  await page.addInitScript(() => {
    window.EventSource = class extends EventTarget {
      close() {}
    } as unknown as typeof EventSource
  })
  const f = await fixture(page, 4, 4)
  await page.getByRole('button', { name: '收起结构', exact: true }).click()
  Object.assign(f.run.nodes[0], { state: 'skipped', execution_id: null })
  Object.assign(f.run.nodes[1], { state: 'unknown', name: 'Repeated name' })
  Object.assign(f.run.nodes[2], {
    state: 'failed',
    name: 'Repeated name',
    reason: 'Current failure',
  })
  f.run.nodes[3].state = 'succeeded'
  f.run.reason = null
  f.run.state = 'failed'
  const trigger = page.getByRole('button', { name: '异常节点 2', exact: true })
  await expect(trigger).toBeVisible()
  await expect(page.locator('.dag-run-meta')).toContainText('失败')
  f.run.state = 'warning'
  f.run.config.dag.results = [f.run.nodes[3].id]
  f.run.reason = 'Synthetic run warning'
  f.run.result = {
    type: 'dag',
    outputs: [
      {
        node_id: f.run.nodes[3].id,
        name: f.run.nodes[3].name,
        state: 'succeeded',
        result: '# Synthetic report',
      },
    ],
  }
  await expect(page.getByRole('heading', { name: 'Synthetic report', exact: true })).toBeVisible()
  await expect(page.locator('.dag-outcome')).toHaveCount(0)
  await expect(page.locator('.dag-run-meta')).toHaveCount(1)
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await expect(trigger).toBeVisible()
  await trigger.press('ArrowDown')
  const menu = page.getByRole('menu')
  const items = menu.getByRole('menuitem')
  await expect(items).toHaveCount(3)
  await expect(items.nth(0)).toHaveAttribute('aria-label', /失败 node-2$/)
  await expect(items.nth(1)).toHaveAttribute('aria-label', /状态待核对 node-1$/)
  await expect(items.nth(2)).toHaveAttribute('aria-label', /已跳过 node-0$/)
  await expect(menu.locator('small')).toHaveCount(2)
  await items.first().press('Escape')
  await expect(trigger).toBeFocused()
  await trigger.click()
  await menu.getByRole('menuitem').first().click()
  await expect(page.getByRole('tab', { name: '轨迹', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  const context = page.locator('.dag-node-process:not([hidden]) > .dag-node-context')
  await expect(context).toHaveText(/Repeated name/)
  await expect(context).toBeFocused()
  await expect(page.locator('.dag-run-structure')).toBeHidden()
  await expect(context.locator('.run-reason')).toHaveAttribute(
    'aria-label',
    '失败原因: Current failure',
  )
  await trigger.click()
  f.run.nodes[1].state = 'succeeded'
  Object.assign(f.run.nodes[2], { state: 'queued', retry_at: '2099-01-01T00:00:00Z' })
  await expect(menu.getByText('无异常节点', { exact: true })).toBeVisible()
  await menu.press('Escape')
  await expect(page.locator('.dag-run-meta')).toBeFocused()
  await expect(page.locator('.dag-node-issues-trigger')).toHaveCount(0)
  expect(f.writes).toEqual([])
})

for (const [width, language, theme] of [
  [1440, 'en', 'light'],
  [390, 'zh', 'dark'],
] as const)
  test(`run status and issue menu layout ${width}px ${language} ${theme}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 })
    await page.addInitScript((language) => {
      localStorage.setItem('wakeit-language', language)
      window.EventSource = class extends EventTarget {
        close() {}
      } as unknown as typeof EventSource
    }, language)
    const f = await fixture(page, 4, 4)
    f.run.nodes[0].state = 'failed'
    f.run.nodes[1].state = 'unknown'
    f.run.nodes[2].state = 'skipped'
    f.run.nodes[3].state = 'succeeded'
    f.run.config.dag.results = [f.run.nodes[3].id]
    f.run.state = 'warning'
    f.run.reason = 'Synthetic independent branch failure'
    f.run.result = {
      type: 'dag',
      outputs: [
        {
          node_id: f.run.nodes[3].id,
          name: f.run.nodes[3].name,
          state: 'succeeded',
          result: '# Synthetic report',
        },
      ],
    }
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    const trigger = page.locator('.dag-node-issues-trigger')
    await expect(trigger).toContainText('2')
    await expect(page.getByRole('heading', { name: 'Synthetic report', exact: true })).toBeVisible()
    await expect(page.locator('.dag-outcome')).toHaveCount(0)
    await expect(page.locator('.run-tabs > .dag-run-meta .status')).toContainText(
      language === 'en' ? 'Warning' : '警告',
    )
    await trigger.click()
    const menu = page.getByRole('menu')
    await expect(menu.getByRole('menuitem')).toHaveCount(3)
    const bounds = await menu.boundingBox()
    expect(bounds!.x).toBeGreaterThanOrEqual(11)
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(width - 11)
    const edges = await menu
      .locator('.dag-issue-head > :last-child, [role="menuitem"] > .status')
      .evaluateAll((els) => els.map((el) => el.getBoundingClientRect().right))
    for (const edge of edges) expect(Math.abs(edge - edges[0])).toBeLessThan(1)
    const gaps = await menu.getByRole('menuitem').evaluateAll((items) =>
      items.map((item) => {
        const name = item.querySelector('.dag-issue-identity')!.getBoundingClientRect()
        const status = item.querySelector('.status')!.getBoundingClientRect()
        return status.left - name.right
      }),
    )
    for (const gap of gaps) expect(gap).toBeGreaterThanOrEqual(8)
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
      width,
    )
    await page.screenshot({ path: `test-results/run-status-${width}-${theme}.png` })
    await menu.press('Escape')
    await expect(trigger).toBeFocused()
    expect(f.writes).toEqual([])
  })

test('tool names stay fully readable beside long summaries on desktop and mobile', async ({
  page,
}) => {
  const { rows } = await fixture(page, 0, 2)
  const tool = rows.at(-1).data
  tool.title = 'Bash'
  tool.input = { command: 'TZ=Asia/Shanghai date && cat /workspace/'.repeat(100) }
  await page.reload()
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 })
    const title = page.locator('[data-trace-id="tool"] .trace-title')
    await expect(title).toHaveText('Bash')
    expect(await title.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true)
    expect(await title.evaluate((element) => getComputedStyle(element).textOverflow)).not.toBe(
      'ellipsis',
    )
    const summary = page.locator('[data-trace-id="tool"] .trace-summary')
    expect(await summary.evaluate((element) => getComputedStyle(element).textOverflow)).toBe(
      'ellipsis',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false)
  }
})
