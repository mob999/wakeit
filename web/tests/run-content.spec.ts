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
  const agents = await (await page.request.get('/api/v1/agents')).json()
  const config = {
    ...original.config,
    name: '仓库检查报告',
    runner: 'dev-machine',
    agent: 'codex',
    cwd: '/projects/中文 repository',
    parameters: { model: 'configured-model' },
  }
  const run = {
    ...original,
    state: 'succeeded',
    source: 'manual',
    config,
    result:
      '# 检查报告\n\n**已完成**\n\n| 项目 | 结果 |\n| --- | --- |\n| 测试 | 通过 |\n\n- 第一项\n- 第二项\n\n```rust\nfn main() {}\n```\n\n[官方文档](https://example.com/docs)\n\n<script>window.mdExecuted = true</script>\n\n[危险链接](javascript:alert(1))',
    execution_snapshot: {
      cwd: '/projects/resolved repository',
      model: 'actual-model',
      parameters: { model: 'actual-model', reasoning: 'high' },
    },
    reason: null,
  }
  const task = {
    id: original.task_id,
    config,
    revision: 1,
    next_at: '2026-10-14T01:00:00Z',
    last_run: { id: run.id, state: 'succeeded', created_at: run.created_at },
  }
  await page.route('**/api/v1/agents', (route) =>
    route.fulfill({
      json: [
        {
          ...agents[0],
          id: 'codex',
          name: 'Codex',
          runner: 'dev-machine',
          runner_id: 'device-id',
          logo: '/agent-logos/codex.svg',
        },
      ],
    }),
  )
  await page.route('**/api/v1/tasks?*', (route) =>
    route.fulfill({ json: { items: [task], total: 1, page: 1, page_size: 20 } }),
  )
  await page.route(`**/api/v1/tasks/${task.id}`, (route) => route.fulfill({ json: task }))
  await page.route('**/api/v1/runs?*', (route) =>
    route.fulfill({ json: { items: [run], total: 1, page: 1, page_size: 20 } }),
  )
  await page.route(`**/api/v1/runs/${run.id}`, (route) => route.fulfill({ json: run }))
  return run
}
test('Tasks and Runs share Runner and Agent identity rows without IDs or dot separators', async ({
  page,
}) => {
  const run = await fixture(page)
  for (const url of ['/tasks', '/runs']) {
    await page.goto(url)
    const row = page.locator('.list-pane .task-row').first()
    await expect(row.locator('.row-title')).toContainText('仓库检查报告')
    await expect(row.locator('.run-device')).toHaveText('dev-machine')
    await expect(row.locator('.run-agent')).toHaveText('Codex')
    await expect(row.locator('.agent-logo img')).toHaveAttribute('src', '/agent-logos/codex.svg')
    await expect(row).not.toContainText(run.id.slice(0, 8))
    await expect(row).not.toContainText('manual')
    await expect(row).not.toContainText('·')
  }
  await page.goto(`/runs?run=${run.id}`)
  await expect(page.locator('.breadcrumbs')).not.toContainText(run.id.slice(0, 8))
  await expect(page.locator('.breadcrumbs')).not.toContainText('·')
  await expect(page.locator('.breadcrumbs')).toContainText('仓库检查报告')
})
test('successful Markdown results render headings, tables and code; metadata lives in details', async ({
  page,
}) => {
  const run = await fixture(page)
  await page.goto(`/runs?run=${run.id}`)
  await expect(page.getByRole('tab', { name: '输出', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await expect(page.getByRole('tablist', { name: '运行内容' }).getByRole('tab')).toHaveText([
    '输出',
    '轨迹',
    '详情',
  ])
  const edges = await page
    .locator('.list-pane .pane-heading, .run-tabs')
    .evaluateAll((headers) => headers.map((header) => header.getBoundingClientRect().bottom))
  expect(edges).toHaveLength(2)
  expect(Math.abs(edges[0] - edges[1])).toBeLessThan(1)
  await expect(page.locator('.run-output-page .run-section-heading')).toHaveText('输出')
  await expect(page.locator('.run-output-page .run-section-heading svg')).toHaveCount(0)
  await expect(page.getByRole('button', { name: '输出', exact: true })).toHaveCount(0)
  await expect(page.locator('.markdown-result h1')).toHaveText('检查报告')
  await expect(page.locator('.markdown-result strong')).toHaveText('已完成')
  await expect(page.locator('.markdown-result table')).toContainText('通过')
  await expect(page.locator('.markdown-result pre code')).toContainText('fn main()')
  await expect(page.locator('.markdown-result a')).toHaveCount(1)
  expect(await page.evaluate(() => (window as any).mdExecuted)).toBeUndefined()
  await expect(
    page.locator('.run-info, .run-execution, .detail-heading .eyebrow, .detail-heading .status'),
  ).toHaveCount(0)
  await expect(page.locator('.run-properties')).not.toBeVisible()
  await page.getByRole('button', { name: '原文', exact: true }).click()
  await expect(page.locator('.result-raw')).toContainText('| 项目 | 结果 |')
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await page.getByRole('tab', { name: '输出', exact: true }).click()
  await expect(page.locator('.result-raw')).toBeVisible()
  await page.getByRole('button', { name: '阅读', exact: true }).click()
  await page.screenshot({ path: 'test-results/runs-result-light.png', fullPage: true })
  await page.getByRole('button', { name: '切换主题' }).click()
  await page.screenshot({ path: 'test-results/runs-result-dark.png', fullPage: true })
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await page.getByRole('button', { name: '执行信息', exact: true }).click()
  await expect(page.locator('.run-properties')).toContainText('/projects/resolved repository')
  await expect(page.locator('.run-properties')).toContainText('actual-model')
  await expect(page.locator('.run-properties')).toContainText('手动')
  await expect(page.locator('.run-properties')).toContainText(run.id)
  await expect(page.locator('.run-details-fold pre')).not.toBeVisible()
  await page.getByText('任务配置', { exact: true }).click()
  await expect(page.locator('.run-details-fold pre')).toBeVisible()
})
test('JSON, empty and truncated results keep their actual form', async ({ page }) => {
  const run = await fixture(page)
  run.result = { summary: '**literal**', count: 2 } as any
  await page.goto(`/runs?run=${run.id}`)
  await expect(page.locator('.result-raw')).toContainText('"summary": "**literal**"')
  await expect(page.locator('.markdown-result')).toHaveCount(0)
  run.result = null as any
  await page.reload()
  await page.getByRole('tab', { name: '输出', exact: true }).click()
  await expect(page.getByText('未返回结果', { exact: true })).toBeVisible()
  await expect(page.getByRole('tabpanel', { name: '输出', exact: true })).toBeVisible()
  run.result = { truncated: true, preview: '# 截断预览' } as any
  await page.reload()
  await expect(page.getByText('结果已截断', { exact: true })).toBeVisible()
  await expect(page.locator('.markdown-result h1')).toHaveText('截断预览')
  await page.getByRole('button', { name: '原文', exact: true }).click()
  await expect(page.locator('.result-raw')).toContainText('"truncated": true')
})

test('recent execution names use available space on desktop and mobile', async ({ page }) => {
  const run = await fixture(page)
  await page.goto(`/tasks?task=${run.task_id}`)
  const row = page.locator('.run-table-row:not(.run-table-head)').first()
  const header = page.locator('.run-table-head')
  for (const [column, cell] of [
    [1, '.run-device'],
    [2, '.run-agent'],
    [3, '.run-table-time'],
  ] as const) {
    const h = (await header
      .locator(':scope > span')
      .nth(column - 1)
      .boundingBox())!
    const d = (await row.locator(cell).last().boundingBox())!
    expect(h.x).toBeCloseTo(d.x, 0)
  }
  const device = (await row.locator('.run-device').boundingBox())!
  const agent = (await row.locator('.run-agent').boundingBox())!
  expect(agent.x - device.x).toBeGreaterThan(120)
  await expect(row).toBeVisible()
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 })
    await expect(row.locator('.run-device')).toHaveText('dev-machine')
    await expect(row.locator('.run-agent')).toHaveText('Codex')
    await expect
      .poll(() =>
        row
          .locator('.run-device > span, .run-agent > span:last-child')
          .evaluateAll((names) => names.every((name) => name.scrollWidth <= name.clientWidth)),
      )
      .toBeTruthy()
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
    ).toBeTruthy()
  }
})
test('polling preserves a chosen tab and mobile results do not overflow the page', async ({
  page,
}) => {
  const run = await fixture(page)
  run.state = 'running'
  run.result = null as any
  await page.goto(`/runs?run=${run.id}`)
  await expect(page.getByRole('tab', { name: '轨迹', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  run.state = 'succeeded'
  run.result = '# 完成' as any
  await expect(page.locator('.task-row.active .status')).toHaveText('成功')
  await expect(page.getByRole('tab', { name: '详情', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await page.setViewportSize({ width: 390, height: 840 })
  await page.getByRole('tab', { name: '输出', exact: true }).click()
  await expect(page.locator('.markdown-result h1')).toHaveText('完成')
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy()
  await page.screenshot({ path: 'test-results/runs-result-mobile.png', fullPage: true })
  run.state = 'unknown'
  run.reason = '外部任务状态需要核对' as any
  await page.reload()
  await expect(page.getByRole('tab', { name: '输出', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await page.getByRole('tab', { name: '轨迹', exact: true }).click()
  await expect(page.locator('.run-context .run-reason')).toHaveAttribute(
    'aria-label',
    /外部任务状态需要核对/,
  )
  await page.getByRole('button', { name: '页面操作', exact: true }).click()
  await expect(page.getByRole('button', { name: '核对状态', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '取消', exact: true })).toHaveCount(0)
})
