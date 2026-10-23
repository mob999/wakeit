import { test, expect, type Page } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
import { loginSession } from './session'
const stack = JSON.parse(
  fs.readFileSync(
    process.env.WAKEIT_TEST_STACK ?? path.resolve('../.local/integration-stack.json'),
    'utf8',
  ),
)
async function fixture(page: Page) {
  await loginSession(page, 'admin', stack.password)
  const original = (
    await (await page.request.get('/api/v1/runs?state=succeeded&page=1&page_size=1')).json()
  ).items[0]
  const run = {
    ...original,
    config: { ...original.config, name: 'Trace fixture' },
    state: 'running',
    started_at: '2026-10-14T01:00:00Z',
    result: null,
    execution_snapshot: {
      parameters: {},
      cwd: '/project',
      model: 'Agent 默认',
      trace_supported: true,
    },
  }
  await page.route('**/api/v1/runs?*', (route) =>
    route.fulfill({ json: { items: [run], total: 1, page: 1, page_size: 20 } }),
  )
  await page.route(`**/api/v1/runs/${run.id}`, (route) => route.fulfill({ json: run }))
  return run
}
const node = {
  id: 'cmd',
  kind: 'command',
  status: 'running',
  title: '检查目录',
  input: { command: 'pwd' },
  output: null,
  occurred_at: '2026-10-14T01:00:00Z',
  started_at: '2026-10-14T01:00:00Z',
}
const event = (id: number, data: any, kind = 'trace') => ({
  id,
  seq: id,
  kind,
  created_at: '2030-01-01T00:00:00Z',
  data,
})
test('Trace history pages independently, merges duplicate and late events, and keeps source timestamps', async ({
  page,
}) => {
  const run = await fixture(page)
  const history = Array.from({ length: 5501 }, (_, i) =>
    event(i + 1, { ...node, output: `progress ${i}` }),
  )
  history.push(event(5502, { ...node, id: 'parallel', title: '并行任务', started_at: undefined }))
  history.push(
    event(5503, {
      ...node,
      id: 'message',
      kind: 'message',
      status: 'succeeded',
      title: '消息',
      output: '# 完成\n\n**中文报告**',
    }),
  )
  const completed = event(6000, {
    ...node,
    status: 'succeeded',
    output: '完成',
    finished_at: '2026-10-14T01:00:02Z',
  })
  const requests: number[] = []
  await page.route(`**/api/v1/runs/${run.id}/events?*`, async (route) => {
    const cursor = Number(new URL(route.request().url()).searchParams.get('after'))
    requests.push(cursor)
    if (cursor === 0) await new Promise((resolve) => setTimeout(resolve, 200))
    await route.fulfill({ json: history.filter((e) => e.id > cursor).slice(0, 500) })
  })
  await page.route(`**/api/v1/runs/${run.id}/stream*`, (route) =>
    route.fulfill({
      contentType: 'text/event-stream',
      body: [completed, completed, event(6001, { ...node, output: 'late progress' })]
        .map((e) => `id: ${e.id}\nevent: run_event\ndata: ${JSON.stringify(e)}\n\n`)
        .join(''),
    }),
  )
  await page.goto(`/runs?run=${run.id}`)
  await expect(page.getByRole('tab', { name: '轨迹', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await expect(page.locator('.trace-row')).toHaveCount(3)
  expect(requests).toContain(5500)
  const command = page.locator('.trace-row').filter({ hasText: '检查目录' })
  await expect(command.locator('.trace-state')).toHaveText('成功')
  await expect(command.locator('.trace-duration')).toHaveText('2.0s')
  await command.locator('summary').click()
  await expect(command.locator('.trace-payload')).toContainText('完成')
  await expect(command.locator('.trace-payload')).not.toContainText('late progress')
  const parallel = page.locator('.trace-row').filter({ hasText: '并行任务' })
  await expect(parallel.locator('.trace-state')).toHaveText('运行中')
  await expect(parallel.locator('.trace-duration')).toHaveCount(0)
  await expect(page.locator('.trace-message h1')).toHaveText('完成')
  await expect(page.locator('.trace-message strong')).toHaveText('中文报告')
  await expect(page.locator('.trace-row > summary')).toHaveCount(3)
  expect(
    await command
      .locator('.trace-payload pre')
      .first()
      .evaluate((e) => getComputedStyle(e).borderWidth),
  ).toBe('0px')
  await expect(page.locator('.result-toolbar, .log-toolbar')).toHaveCount(0)
  await page.screenshot({ path: 'test-results/runs-trace-light.png', fullPage: true })
  await page.getByRole('button', { name: '切换主题' }).click()
  await page.screenshot({ path: 'test-results/runs-trace-dark.png', fullPage: true })
  await page.setViewportSize({ width: 390, height: 900 })
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy()
  await page.screenshot({ path: 'test-results/runs-trace-mobile.png', fullPage: true })
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  run.state = 'succeeded'
  run.result = '# 最终报告' as any
  await expect
    .poll(async () => (await page.request.get(`/api/v1/runs/${run.id}`)).status())
    .toBe(200)
  await expect(page.getByRole('tab', { name: '详情', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await expect(page.locator('.list-pane .status')).toHaveText('成功')
  await page.getByRole('tab', { name: '轨迹', exact: true }).click()
  await expect(parallel.locator('.trace-state')).toHaveText('未记录结束')
})
test('unsupported, historic, waiting, truncated and expired process states stay distinct', async ({
  page,
}) => {
  const run = await fixture(page)
  let rows: any[] = []
  await page.route(`**/api/v1/runs/${run.id}/events?*`, (route) => route.fulfill({ json: rows }))
  await page.route(`**/api/v1/runs/${run.id}/stream*`, (route) =>
    route.fulfill({ contentType: 'text/event-stream', body: ': heartbeat\n\n' }),
  )
  for (const [support, notice, text] of [
    [false, null, '等待执行输出…'],
    [undefined, null, '此运行未记录过程'],
    [true, null, '等待 Agent 事件…'],
    [true, 'truncated', '过程已截断'],
    [true, 'expired', '过程已过期'],
  ] as const) {
    run.execution_snapshot.trace_supported = support as any
    rows = notice ? [event(1, { status: notice }, 'trace_status')] : []
    await page.goto(`/runs?run=${run.id}`)
    await page.getByRole('tab', { name: '轨迹', exact: true }).click()
    if (support === false)
      await expect(page.locator('.run-execution-page .logs')).toContainText(text)
    else if (notice === 'truncated') {
      await page.getByRole('button', { name: '过程记录提示', exact: true }).focus()
      await expect(page.getByRole('tooltip')).toContainText('未提供具体原因')
    } else await expect(page.getByText(text, { exact: true })).toBeVisible()
  }
})

test('output and trace are sibling tabs, preserve local folds and support keyboard navigation', async ({
  page,
}) => {
  const run = await fixture(page)
  await page.route(`**/api/v1/runs/${run.id}/events?*`, (route) =>
    route.fulfill({ json: [event(1, node)] }),
  )
  await page.route(`**/api/v1/runs/${run.id}/stream*`, (route) =>
    route.fulfill({ contentType: 'text/event-stream', body: ': heartbeat\n\n' }),
  )
  await page.goto(`/runs?run=${run.id}`)
  const tabs = page.getByRole('tablist', { name: '运行内容' })
  await expect(tabs.getByRole('tab')).toHaveText(['输出', '轨迹', '详情'])
  await expect(tabs.getByRole('tab', { name: '轨迹', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await expect(page.locator('.run-execution-page .run-section-heading')).toHaveText('轨迹')
  await expect(page.locator('.run-execution-page .run-section-heading svg')).toHaveCount(0)
  await page.locator('.trace-operation > summary').click()
  run.state = 'succeeded'
  await expect(page.getByRole('tab', { name: '输出', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await expect(page.getByText('未返回结果', { exact: true })).toBeVisible()
  run.result = '# 最终结果' as any
  await expect(page.locator('.run-result-panel h1')).toHaveText('最终结果')
  await page.getByRole('tab', { name: '输出', exact: true }).press('ArrowRight')
  await expect(page.getByRole('tab', { name: '轨迹', exact: true })).toBeFocused()
  await expect(page.locator('.trace-payload')).toBeVisible()
  await page.getByRole('tab', { name: '轨迹', exact: true }).press('End')
  await expect(page.getByRole('tab', { name: '详情', exact: true })).toBeFocused()
  await page.getByRole('button', { name: '执行信息', exact: true }).click()
  await page.getByRole('tab', { name: '详情', exact: true }).press('Home')
  await expect(page.getByRole('tab', { name: '输出', exact: true })).toBeFocused()
  await page.getByRole('tab', { name: '轨迹', exact: true }).click()
  await page.waitForTimeout(2300)
  await expect(page.getByRole('tab', { name: '轨迹', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await expect(page.locator('.trace-payload')).toBeVisible()
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await expect(page.getByRole('button', { name: '执行信息', exact: true })).toHaveAttribute(
    'aria-expanded',
    'true',
  )
})
test('failure reasons, queued runs and non-Trace output do not become reports', async ({
  page,
}) => {
  const run = await fixture(page)
  await page.route(`**/api/v1/runs/${run.id}/events?*`, (route) => route.fulfill({ json: [] }))
  await page.route(`**/api/v1/runs/${run.id}/stream*`, (route) =>
    route.fulfill({ contentType: 'text/event-stream', body: ': heartbeat\n\n' }),
  )
  run.state = 'queued'
  run.started_at = null as any
  await page.goto(`/runs?run=${run.id}`)
  await expect(page.locator('.run-execution-page .run-waiting')).toHaveText('等待执行')
  await expect(page.locator('.run-execution-page .run-section-heading')).toHaveText('轨迹')
  for (const state of ['failed', 'timed_out', 'cancelled', 'interrupted', 'unknown', 'skipped']) {
    run.state = state
    run.reason = `实际原因 ${state}`
    run.result = '# 部分输出' as any
    await page.reload()
    await page.getByRole('tab', { name: '轨迹', exact: true }).click()
    await expect(page.locator('.run-context .run-reason')).toHaveAttribute(
      'aria-label',
      new RegExp(`实际原因 ${state}`),
    )
    await page.getByRole('tab', { name: '输出', exact: true }).click()
    await expect(page.locator('.run-result-panel h1')).toHaveText('部分输出')
  }
  run.state = 'running'
  run.result = null
  run.reason = null
  run.execution_snapshot.trace_supported = false
  await page.route(`**/api/v1/runs/${run.id}/stream*`, (route) =>
    route.fulfill({
      contentType: 'text/event-stream',
      body: `event: run_event\ndata: ${JSON.stringify(event(9, { message: '外部任务正在处理' }, 'progress'))}\n\n`,
    }),
  )
  await page.reload()
  await expect(page.getByRole('tab', { name: '轨迹', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await expect(page.locator('.run-execution-page .logs')).toContainText('外部任务正在处理')
  await page.getByRole('tab', { name: '输出', exact: true }).click()
  await expect(page.locator('.run-output-page .run-waiting')).toHaveText('等待执行输出…')
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await expect(page.locator('.run-diagnostic-page .logs')).toContainText('外部任务正在处理')
})

test('logs reconnect without duplicate output and keep follow and information folds', async ({
  page,
}) => {
  const run = await fixture(page)
  run.execution_snapshot.trace_supported = false
  await page.route(`**/api/v1/runs/${run.id}/events?*`, (route) => route.fulfill({ json: [] }))
  let connections = 0
  await page.route(`**/api/v1/runs/${run.id}/stream*`, (route) => {
    connections++
    const records = [
      event(1, { text: 'first line\n' }, 'log'),
      ...(connections > 1 ? [event(2, { text: 'second line\n' }, 'log')] : []),
    ]
    return route.fulfill({
      contentType: 'text/event-stream',
      body: records
        .map((e) => `id: ${e.id}\nevent: run_event\ndata: ${JSON.stringify(e)}\n\n`)
        .join(''),
    })
  })
  await page.goto(`/runs?run=${run.id}`)
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  const panel = page.locator('.run-diagnostic-page')
  await expect(panel.locator('.logs')).toContainText('first line')
  await panel.getByLabel('跟随输出', { exact: true }).uncheck()
  await page.getByRole('button', { name: '执行信息', exact: true }).click()
  await expect(panel.getByRole('button', { name: '重新连接日志' })).toBeVisible()
  await panel.getByRole('button', { name: '重新连接日志' }).click()
  await expect(panel.locator('.logs')).toContainText('second line')
  expect((await panel.locator('.logs').innerText()).match(/first line/g)).toHaveLength(1)
  await page.getByRole('tab', { name: '轨迹', exact: true }).click()
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await expect(panel.getByLabel('跟随输出', { exact: true })).not.toBeChecked()
  await expect(page.getByRole('button', { name: '执行信息', exact: true })).toHaveAttribute(
    'aria-expanded',
    'true',
  )
})
