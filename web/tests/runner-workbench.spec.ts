import { loginSession } from './session'
import { test, expect, type Page } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
const stack = JSON.parse(
  fs.readFileSync(
    process.env.WAKEIT_TEST_STACK ?? path.resolve('../.local/integration-stack.json'),
    'utf8',
  ),
)
async function login(page: Page) {
  await loginSession(page, 'admin', stack.password)
}
test('Agent search locates the device adapter and creates a task with both selections', async ({
  page,
}) => {
  await login(page)
  await page.getByRole('button', { name: '搜索与筛选' }).click()
  const search = page.getByRole('dialog', { name: '搜索', exact: true })
  await search
    .locator('.search-scopes')
    .getByRole('button', { name: 'Agent', exact: true })
    .click()
  await search.getByLabel('搜索内容').fill('mock-sync')
  await search.getByRole('button', { name: 'mock-sync test-device', exact: true }).click()
  await expect(page).toHaveURL(/\/runners\?.*section=agents.*agent=mock-sync/)
  await expect(page.locator('.header-breadcrumbs [role="heading"]')).toHaveText('test-device')
  await expect(page.locator('.agent-details')).toContainText('支持查询')
  await expect(page.getByRole('button', { name: '收起 mock-sync', exact: true })).toHaveAttribute(
    'aria-expanded',
    'true',
  )
  await page.reload()
  await expect(page.locator('.agent-details')).toContainText('支持查询')
  const crumbs = page.getByRole('navigation', { name: '层级导航' })
  await expect(crumbs).toContainText('设备')
  await expect(crumbs).toContainText('test-device')
  await expect(crumbs).toContainText('mock-sync')
  await crumbs.getByRole('button', { name: 'Agent', exact: true }).click()
  await expect(page.locator('.agent-details')).toHaveCount(0)
  await page.getByRole('button', { name: '展开 mock-sync', exact: true }).click()
  await expect(page.getByRole('button', { name: '适配模板', exact: true })).toHaveCount(0)
  await page
    .locator('.runner-agent-item')
    .filter({ has: page.getByRole('button', { name: '收起 mock-sync', exact: true }) })
    .getByRole('button', { name: '创建任务', exact: true })
    .click()
  const editor = page.getByRole('dialog', { name: '新建任务', exact: true })
  await expect(editor.getByLabel('Runner', { exact: true })).toContainText('test-device')
  await expect(editor.getByLabel('Agent', { exact: true })).toContainText('mock-sync')
  await expect(editor.getByLabel('任务名称')).toHaveValue('')
  await expect(editor.getByLabel('指令')).toHaveValue('')
  await expect(editor.getByRole('combobox', { name: '模型', exact: true })).toContainText(
    '先选择工作目录',
  )
  await expect(
    page.locator('.navigation').getByRole('link', { name: 'Agent', exact: true }),
  ).toHaveCount(0)
})
test('Runner paging, deep links and polling preserve selected devices', async ({ page }) => {
  await login(page)
  const original = (await (await page.request.get('/api/v1/runners')).json()).find(
    (r: any) => r.name === 'test-device',
  )
  const devices = Array.from({ length: 26 }, (_, i) => ({
    ...original,
    id: `fixture-${i}`,
    name: `device-${String(i).padStart(2, '0')}`,
  }))
  await page.route('**/api/v1/runners', (route) => route.fulfill({ json: devices }))
  await page.getByRole('link', { name: '设备', exact: true }).click()
  const list = page.locator('.list-pane')
  await expect(list.locator('.runner-row')).toHaveCount(20)
  await list.getByRole('button', { name: '下一页' }).click()
  await expect(list.locator('.runner-row')).toHaveCount(6)
  await expect(page.locator('.header-breadcrumbs [role="heading"]')).toHaveText('device-20')
  await page.goto('/runners?runner=fixture-25&section=overview')
  await expect(list.locator('.runner-row.active')).toContainText('device-25')
  await expect(list.getByRole('navigation', { name: '分页' })).toContainText('2 / 2')
  const polled = page.waitForResponse((r) => r.url().endsWith('/api/v1/runners'))
  await polled
  await expect(page.locator('.header-breadcrumbs [role="heading"]')).toHaveText('device-25')
})
test('Agent and device filters intersect while per-device capabilities stay separate', async ({
  page,
}) => {
  await login(page)
  const original = (await (await page.request.get('/api/v1/runners')).json()).find(
    (r: any) => r.name === 'test-device',
  )
  const a = original.agents.find((a: any) => a.id === 'mock-sync')
  const devices = [
    {
      ...original,
      id: 'online-one',
      name: 'online-one',
      online: true,
      agents: [{ ...a, version: '1.2', models_supported: true }],
    },
    {
      ...original,
      id: 'offline-two',
      name: 'offline-two',
      online: false,
      agents: [{ ...a, version: '2.3', models_supported: false, mode: 'async' }],
    },
  ]
  await page.route('**/api/v1/runners', (route) => route.fulfill({ json: devices }))
  await page.route('**/api/v1/agents', (route) =>
    route.fulfill({
      json: devices.flatMap((r) =>
        r.agents.map((a: any) => ({ ...a, runner: r.name, runner_id: r.id })),
      ),
    }),
  )
  await page.getByRole('button', { name: '搜索与筛选' }).click()
  const search = page.getByRole('dialog', { name: '搜索', exact: true })
  for (const name of ['设备', 'Agent'])
    await search.locator('.search-scopes').getByRole('button', { name, exact: true }).click()
  await search.getByLabel('设备状态').selectOption('online')
  await search.getByLabel('模式').selectOption('sync')
  await search.getByRole('button', { name: '应用', exact: true }).click()
  await expect(page.locator('.runner-row')).toHaveCount(1)
  await expect(page.locator('.runner-row')).toContainText('online-one')
  await page.getByRole('button', { name: '展开 mock-sync', exact: true }).click()
  await expect(page.locator('.agent-details')).toContainText('支持查询')
  await expect(page.locator('.agent-version')).toHaveText('1.2')
  await page.getByRole('button', { name: '搜索与筛选' }).click()
  await search.getByRole('button', { name: '清除筛选', exact: true }).click()
  await page.keyboard.press('Escape')
  await page.locator('.runner-row').filter({ hasText: 'offline-two' }).click()
  await page.getByRole('button', { name: '展开 mock-sync', exact: true }).click()
  await expect(page.locator('.agent-details')).toContainText('异步')
  await expect(page.locator('.agent-details')).toContainText('手填')
  await expect(page.locator('.agent-version')).toHaveText('2.3')
  await page.screenshot({ path: 'test-results/runners-agents-light.png', fullPage: true })
  await page.getByRole('button', { name: '切换主题' }).click()
  await page.screenshot({ path: 'test-results/runners-agents-dark.png', fullPage: true })
})
test('all four workbenches switch between list and detail on narrow screens', async ({ page }) => {
  await login(page)
  await page.setViewportSize({ width: 390, height: 840 })
  for (const [name, row] of [
    ['任务', '.task-row'],
    ['运行记录', '.task-row'],
    ['设备', '.runner-row'],
    ['设置', '.settings-category'],
  ]) {
    await page.getByRole('link', { name, exact: true }).click()
    await expect(page.locator('.list-pane')).toBeVisible()
    await expect(page.locator('.detail-pane')).not.toBeVisible()
    await page.locator(`.list-pane ${row}`).first().click()
    await expect(page.locator('.detail-pane')).toBeVisible()
    await expect(page.locator('.list-pane')).not.toBeVisible()
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
    ).toBeTruthy()
    await page
      .getByRole('navigation', { name: '层级导航' })
      .getByRole('button', { name, exact: true })
      .click()
    await expect(page.locator('.list-pane')).toBeVisible()
  }
  await page.getByRole('link', { name: '设置', exact: true }).click()
  await page.getByRole('button', { name: '工作区', exact: true }).click()
  const original = await page.getByLabel('日志保留', { exact: true }).inputValue()
  await page.getByLabel('日志保留', { exact: true }).fill(String(Number(original) + 1))
  await page
    .getByRole('navigation', { name: '层级导航' })
    .getByRole('button', { name: '设置', exact: true })
    .click()
  const guard = page.getByRole('dialog', { name: '有未保存的更改' })
  await expect(guard).toBeVisible()
  await guard.getByRole('button', { name: '继续编辑' }).click()
  await expect(page.getByLabel('日志保留', { exact: true })).toHaveValue(
    String(Number(original) + 1),
  )
  await page
    .getByRole('navigation', { name: '层级导航' })
    .getByRole('button', { name: '设置', exact: true })
    .click()
  await guard.getByRole('button', { name: '放弃更改' }).click()
  await expect(page.locator('.list-pane')).toBeVisible()
})

test('full-width Agent table shows single-line names, icon actions and logo fallback', async ({
  page,
}) => {
  await login(page)
  const original = (await (await page.request.get('/api/v1/runners')).json()).find(
    (r: any) => r.name === 'test-device',
  )
  const base = original.agents[0]
  const entries = [
    { ...base, id: 'pi', name: 'Pi', logo: '/agent-logos/pi.svg' },
    { ...base, id: 'codex', name: 'Codex', logo: '/agent-logos/codex.svg' },
    { ...base, id: 'opencode', name: 'OpenCode', logo: '/agent-logos/opencode.svg' },
    { ...base, id: 'custom', name: 'Custom', logo: null },
    { ...base, id: 'broken', name: 'Broken', logo: '/agent-logos/missing.svg' },
  ]
  await page.route('**/api/v1/runners', (route) =>
    route.fulfill({ json: [{ ...original, agents: entries }] }),
  )
  await page.setViewportSize({ width: 2200, height: 1000 })
  await page.goto(`/runners?runner=${original.id}&section=agents`)
  const table = page.locator('.runner-agents-table')
  await expect(table.getByText('操作', { exact: true })).toBeVisible()
  await expect(page.locator('.agent-display-name')).toHaveText([
    'Pi',
    'Codex',
    'OpenCode',
    'Custom',
    'Broken',
  ])
  await expect(page.locator('.agent-expand small')).toHaveCount(0)
  await expect(page.locator('.agent-logo > span')).toHaveText(['C', 'B'])
  await expect
    .poll(() =>
      page
        .locator('.agent-logo img')
        .evaluateAll((imgs) =>
          imgs.every(
            (img) =>
              (img as HTMLImageElement).complete && (img as HTMLImageElement).naturalWidth > 0,
          ),
        ),
    )
    .toBe(true)
  const box = await table.boundingBox(),
    content = await page.locator('.detail-content').boundingBox()
  expect(box!.width).toBeGreaterThan(960)
  expect(box!.width).toBe(content!.width)
  const create = page.locator('.agent-create-button').first()
  await expect(create).toHaveAttribute('aria-label', '创建任务')
  await expect(create).toHaveText('')
  expect(await create.evaluate((el) => getComputedStyle(el).borderWidth)).toBe('0px')
  await page.screenshot({ path: 'test-results/agent-table-light.png', fullPage: true })
  await page.getByRole('button', { name: '切换主题' }).click()
  await page.screenshot({ path: 'test-results/agent-table-dark.png', fullPage: true })
  await page.setViewportSize({ width: 390, height: 840 })
  await expect(table.getByText('操作', { exact: true })).toBeVisible()
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy()
  await create.click()
  const editor = page.getByRole('dialog', { name: '新建任务', exact: true })
  await expect(editor.getByLabel('Agent', { exact: true })).toContainText('Pi')
})
