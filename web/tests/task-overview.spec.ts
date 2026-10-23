import { test, expect, type Page } from '@playwright/test'
import { mockWorkspace, workspacePage, apiPath } from './workspace'
import { languageAction } from './actions'

async function fixture(page: Page) {
  const runner = '22222222-2222-4222-8222-222222222222'
  const prompt = Array.from(
    { length: 30 },
    (_, i) => `Line ${i + 1}: preserve the original instruction.`,
  ).join('\n')
  const config: any = {
    version: 1,
    id: 'overview',
    name: 'Synthetic overview',
    runner,
    agent: 'pi',
    cwd: '/workspace/research',
    prompt,
    parameters: { model: 'configured-model' },
    session: null,
    enabled: true,
    schedule: { cron: '0 9 * * *', timezone: 'Asia/Shanghai' },
    execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
  }
  const task = {
    id: config.id,
    config,
    revision: 1,
    next_at: '2026-10-24T01:00:00Z',
    archived_at: null,
  }
  const writes: string[] = []
  await page.route('**/api/v1/**', async (route) => {
    if (await mockWorkspace(route, 'viewer')) return
    const path = apiPath(new URL(route.request().url()).pathname)
    if (route.request().method() !== 'GET') writes.push(path)
    let json: any = []
    if (path === '/session') json = { id: 'preview', username: 'preview', role: 'viewer' }
    else if (path === '/meta') json = { version: '0.3.2' }
    else if (path === '/settings') json = { timezone: 'Asia/Shanghai' }
    else if (path === '/tasks') json = { items: [task], total: 1 }
    else if (path.startsWith('/tasks/')) json = task
    else if (path === '/agents')
      json = [
        { id: 'pi', name: 'Pi', runner_id: runner, runner: 'Preview workstation', available: true },
      ]
    else if (path === '/runners')
      json = [{ id: runner, name: 'Preview workstation', agents: [], revoked: false }]
    else if (path === '/runs')
      json = {
        items: Array.from({ length: 5 }, (_, i) => ({
          id: `00000000-0000-4000-8000-${String(i + 1).padStart(12, '0')}`,
          task_id: config.id,
          config,
          state: 'succeeded',
          started_at: '2026-10-23T01:00:00Z',
          created_at: '2026-10-23T01:00:00Z',
        })),
        total: 5,
      }
    return route.fulfill({ json })
  })
  await page.goto(workspacePage('/tasks?task=overview'))
  return { task, writes, prompt }
}

test('one task frame preserves full instructions, current settings, parameter placeholders and recent runs', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  const f = await fixture(page)
  const card = page.locator('.task-overview')
  await expect(page.locator('.detail-scroll > .content-card')).toHaveCount(1)
  await expect(card.locator('.content-card')).toHaveCount(0)
  await expect(card.locator('.task-overview-context')).toContainText('Preview workstation')
  await expect(card.locator('.task-overview-context')).toContainText('configured-model')
  await expect(
    card.locator('dt').filter({ hasText: '其他参数' }).locator('xpath=following-sibling::dd[1]'),
  ).toHaveText('—')
  await expect(card).not.toContainText('执行策略')
  await expect(card.locator('.task-prompt')).toHaveCSS('max-height', '240px')
  await card.getByRole('button', { name: '复制指令', exact: true }).click()
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(f.prompt)
  await card.getByRole('button', { name: '展开全文', exact: true }).click()
  await expect(card.locator('.task-prompt')).not.toHaveClass(/is-preview/)
  await card.getByRole('button', { name: '收起全文', exact: true }).click()
  await expect(card.locator('.task-overview-history > a')).toHaveCount(5)
  const heights = await card
    .locator('.task-overview-history > .run-table-row')
    .evaluateAll((rows) => rows.map((row) => row.getBoundingClientRect().height))
  expect(heights).toEqual([44, 48, 48, 48, 48, 48])
  f.task.config.prompt = 'A short instruction'
  f.task.config.parameters = { model: 'changed-model', reasoning: 'high' }
  await expect(card.locator('.task-prompt')).toHaveText('A short instruction')
  await expect(card.getByRole('button', { name: '展开全文', exact: true })).toHaveCount(0)
  await card.locator('.task-overview-parameters > summary').click()
  await expect(card.locator('.task-overview-parameters code')).toContainText('"reasoning": "high"')
  await expect(card.locator('.task-overview-parameters code')).not.toContainText('model')
  expect(f.writes).toEqual([])
})

test('manual scheduling, translated controls and narrow history retain their information', async ({
  page,
}) => {
  const f = await fixture(page)
  f.task.config.schedule = null
  await expect(page.locator('.task-overview-schedule')).toContainText('手动执行')
  await expect(page.locator('.task-overview-next')).toHaveCount(0)
  for (const width of [1440, 1024, 390]) {
    await page.setViewportSize({ width, height: 900 })
    const history = page.locator('.task-overview-history')
    await expect(
      history.getByRole('button', { name: '查看全部执行历史', exact: true }),
    ).toBeVisible()
    await expect(history.locator('.run-device').first()).toBeVisible()
    await expect(history.locator('.run-agent').first()).toBeVisible()
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  }
  await (await languageAction(page, '切换到英文')).click()
  await expect(page.locator('.task-overview-schedule')).toContainText('Manual')
  await expect(page.getByRole('button', { name: 'Copy prompt', exact: true })).toBeVisible()
  await page.getByRole('button', { name: 'View all executions', exact: true }).click()
  await expect(page.locator('.run-table-head')).toContainText('Run history')
  expect(f.writes).toEqual([])
})
