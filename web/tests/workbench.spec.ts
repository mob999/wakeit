import { selectChoice } from './choices'
import { loginSession } from './session'
import { test, expect, type Page } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
test.use({ timezoneId: 'Asia/Shanghai' })
const stack = JSON.parse(
  fs.readFileSync(
    process.env.WAKEIT_TEST_STACK ?? path.resolve('../.local/integration-stack.json'),
    'utf8',
  ),
)
async function login(page: Page, user = 'admin', password = stack.password) {
  await loginSession(page, user, password)
}
async function selectDirectory(page: Page, dialog: any) {
  await dialog.getByRole('button', { name: '选择目录', exact: true }).click()
  const picker = page.getByRole('dialog', { name: '选择工作目录', exact: true })
  await picker.getByRole('button', { name: '选择文件夹', exact: true }).click()
  await expect(picker).not.toBeVisible()
}
test('workspace timezone initializes the form and formats schedule previews', async ({ page }) => {
  await login(page)
  const settings = await (await page.request.get('/api/v1/settings')).json()
  const headers = { 'X-Wakeit-Request': '1' }
  try {
    expect(
      (
        await page.request.put('/api/v1/settings', {
          data: { ...settings, timezone: 'UTC' },
          headers,
        })
      ).ok(),
    ).toBeTruthy()
    await page.reload()
    await page
      .locator('.pane-heading')
      .getByRole('button', { name: '新建任务', exact: true })
      .click()
    const dialog = page.getByRole('dialog')
    await dialog.getByLabel('任务名称', { exact: true }).fill('timezone preview')
    await dialog.getByLabel('指令', { exact: true }).fill('preview')
    await selectChoice(page, dialog, 'Runner', 'test-device')
    await selectChoice(page, dialog, 'Agent', 'mock-sync')
    await selectDirectory(page, dialog)
    await dialog.getByRole('button', { name: '下一步', exact: true }).click()
    await expect(dialog.getByLabel('时区', { exact: true })).toHaveValue('UTC')
    await expect(dialog.locator('.preview')).toContainText('09:00:00 UTC')
  } finally {
    await page.request.put('/api/v1/settings', { data: settings, headers })
  }
})

test('create, edit, pause, trigger and inspect a real Agent run', async ({ page }) => {
  await login(page)
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByLabel('任务名称', { exact: true }).fill('WebUI smoke')
  await selectChoice(page, dialog, 'Runner', 'test-device')
  await selectChoice(page, dialog, 'Agent', 'mock-sync')
  await selectDirectory(page, dialog)
  await dialog.getByRole('textbox', { name: '指令', exact: true }).fill('WebUI end-to-end result')
  await dialog.getByRole('button', { name: '下一步', exact: true }).click()
  await selectChoice(page, dialog, '类型', 'manual')
  await dialog.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(dialog).not.toBeVisible()
  await page
    .getByRole('button', { name: /WebUI smoke/ })
    .first()
    .click()
  await expect(page.getByRole('heading', { name: 'WebUI smoke', exact: true })).toBeVisible()
  await page.getByRole('button', { name: '暂停任务', exact: true }).click()
  await expect(page.getByRole('button', { name: '恢复任务', exact: true })).toBeVisible()
  await page.getByRole('button', { name: '编辑', exact: true }).click()
  await expect(dialog.getByRole('button', { name: 'YAML', exact: true })).toHaveCount(0)
  await expect(dialog.getByLabel('Agent', { exact: true })).toContainText('mock-sync')
  await dialog
    .getByRole('textbox', { name: '指令', exact: true })
    .fill('Updated WebUI end-to-end result')
  await dialog.getByRole('button', { name: '下一步', exact: true }).click()
  await dialog.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(dialog).not.toBeVisible()
  await expect(page.getByRole('button', { name: '恢复任务', exact: true })).toBeVisible()
  await page.getByRole('button', { name: '立即执行', exact: true }).click()
  await expect(page.locator('.task-row.active .status')).toHaveText('成功', { timeout: 20000 })
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await expect(page.locator('.run-diagnostic-page .logs')).toContainText(
    'Updated WebUI end-to-end result',
  )
  await page.getByRole('tab', { name: '执行', exact: true }).click()
  await expect(page.locator('.result')).toContainText('Updated WebUI end-to-end result')
  await page.screenshot({ path: 'test-results/wakeit-run-result.png', fullPage: true })
})

test('Runner capability details and enrollment', async ({ page }) => {
  await login(page)
  await expect(page.getByRole('link', { name: 'Agent', exact: true })).toHaveCount(0)
  await page.getByRole('link', { name: '设备', exact: true }).click()
  await page.locator('.runner-row').filter({ hasText: 'test-device' }).click()
  await expect(page.getByRole('heading', { name: 'test-device', exact: true })).toBeVisible()
  await page.getByRole('tab', { name: /^Agent/ }).click()
  await expect(page.locator('.runner-agents-table')).toContainText('mock-async')
  await expect(page.getByRole('button', { name: '适配模板', exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '连接设备', exact: true }).click()
  await page.getByRole('button', { name: '生成注册命令', exact: true }).click()
  await expect(page.getByRole('dialog')).toContainText('--token-env WAKEIT_ENROLL_TOKEN')
})

test('Viewer can inspect runs but cannot edit or trigger', async ({ page }) => {
  await login(page, 'viewer', 'wakeit-role-test-password')
  await expect(page.getByRole('button', { name: '新建任务', exact: true })).toHaveCount(0)
  await expect(page.getByRole('button', { name: '立即执行', exact: true })).toHaveCount(0)
  await page.getByRole('link', { name: '运行', exact: true }).click()
  await expect(page.getByRole('tab', { name: '详情', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '重试', exact: true })).toHaveCount(0)
  await page.getByRole('link', { name: '设置', exact: true }).click()
  await expect(page.getByRole('heading', { name: '成员', exact: true })).toHaveCount(0)
  await expect(page.getByRole('heading', { name: /^API Key/ })).toBeVisible()
  await page.getByRole('button', { name: '搜索与筛选' }).click()
  const search = page.getByRole('dialog', { name: '搜索', exact: true })
  await search
    .locator('.search-scopes')
    .getByRole('button', { name: '设置', exact: true })
    .click()
  await expect(search.getByRole('button', { name: '成员', exact: true })).toHaveCount(0)
  await expect(search.getByRole('button', { name: '工作区', exact: true })).toHaveCount(0)
  await expect(search.getByRole('button', { name: '个人 API Key', exact: true })).toBeVisible()
})

test('invite acceptance creates a Viewer account', async ({ page, browser }) => {
  await login(page)
  await page.getByRole('link', { name: '设置', exact: true }).click()
  await page.getByRole('button', { name: '成员', exact: true }).click()
  await page.getByRole('button', { name: '邀请成员', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '邀请成员', exact: true })
  await dialog.getByLabel('邀请角色').selectOption('viewer')
  await dialog.getByRole('button', { name: '生成链接', exact: true }).click()
  const link = await dialog.getByLabel('邀请链接', { exact: true }).inputValue()
  const context = await browser.newContext()
  const invited = await context.newPage()
  await invited.goto(link)
  const username = `invited-${Date.now()}`
  await invited.getByLabel('用户名', { exact: true }).fill(username)
  await invited.getByLabel('密码').fill('wakeit-invited-password')
  await invited.getByRole('button', { name: '创建账号', exact: true }).click()
  await expect(invited.getByRole('button', { name: '登录工作区' })).toBeVisible()
  await invited.getByLabel('用户名', { exact: true }).fill(username)
  await invited.getByLabel('密码', { exact: true }).fill('wakeit-invited-password')
  await invited.getByRole('button', { name: '登录工作区' }).click()
  await expect(invited.getByRole('heading', { name: '动态', exact: true })).toBeVisible()
  await expect(invited.getByRole('button', { name: '新建任务', exact: true })).toHaveCount(0)
  await context.close()
})

test('theme switching and API Key lifecycle', async ({ page, request }) => {
  await login(page)
  await page.getByRole('button', { name: '切换主题', exact: true }).click()
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark')
  await page.screenshot({ path: 'test-results/wakeit-tasks-dark.png', fullPage: true })
  await page.getByRole('link', { name: '设置', exact: true }).click()
  await page.getByRole('button', { name: 'API Key', exact: true }).click()
  const name = `browser-token-${Date.now()}`
  await page.getByRole('button', { name: '创建 API Key', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '创建 API Key', exact: true })
  await dialog.getByLabel('名称').fill(name)
  await dialog.getByRole('button', { name: '创建', exact: true }).click()
  const token = await dialog.getByLabel('API Key', { exact: true }).inputValue()
  expect(
    (await request.get('/api/v1/tasks', { headers: { Authorization: `Bearer ${token}` } })).ok(),
  ).toBeTruthy()
  await dialog.getByRole('button', { name: '关闭', exact: true }).last().click()
  await page
    .locator('.settings-token-row')
    .filter({ hasText: name })
    .getByRole('button', { name: '撤销', exact: true })
    .click()
  await page
    .getByRole('dialog', { name: '撤销 API Key', exact: true })
    .getByRole('button', { name: '撤销', exact: true })
    .click()
  await expect
    .poll(async () =>
      (
        await request.get('/api/v1/tasks', { headers: { Authorization: `Bearer ${token}` } })
      ).status(),
    )
    .toBe(401)
})

test('header, directory browser, discovered models and parameter drafts across pages', async ({
  page,
}) => {
  await login(page)
  const header = page.locator('.topbar')
  await expect(header.locator('.brand-version')).toHaveText('0.1.0')
  await expect(header).not.toContainText('工作区')
  await expect(header).not.toContainText('Scheduling for agents')
  await expect(page.locator('.nav-bottom .version')).toHaveCount(0)
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '新建任务', exact: true })
  await dialog.getByLabel('任务名称', { exact: true }).fill('Model selection test')
  await selectChoice(page, dialog, 'Runner', 'test-device')
  await selectDirectory(page, dialog)
  await selectChoice(page, dialog, 'Agent', 'mock-sync')
  await expect(dialog.getByRole('combobox', { name: '模型', exact: true })).toBeVisible()
  await expect(dialog.getByRole('combobox', { name: '模型', exact: true })).toContainText(
    '使用默认配置',
  )
  await dialog.getByRole('combobox', { name: '模型', exact: true }).click()
  await page.getByLabel('搜索模型').fill('Model B')
  await page.getByRole('option', { name: /Model B/ }).click()
  await dialog.locator('.advanced-settings:visible > summary').click()
  await dialog.getByRole('textbox', { name: '推理强度', exact: true }).fill('high')
  await dialog.getByLabel('指令', { exact: true }).fill('model snapshot')
  await dialog.getByRole('button', { name: '下一步', exact: true }).click()
  await dialog.getByRole('button', { name: '上一步', exact: true }).click()
  await expect(dialog.getByRole('combobox', { name: '模型', exact: true })).toContainText('Model B')
  await dialog.getByRole('textbox', { name: '指令', exact: true }).fill('model snapshot')
  await dialog.getByRole('button', { name: '下一步', exact: true }).click()
  await selectChoice(page, dialog, '类型', 'manual')
  await dialog.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(dialog).not.toBeVisible()
})

test('models failure and empty response never reuse the old list', async ({ page }) => {
  await login(page)
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '新建任务', exact: true })
  await selectChoice(page, dialog, 'Runner', 'test-device')
  await selectDirectory(page, dialog)
  await selectChoice(page, dialog, 'Agent', 'mock-sync')
  await expect(dialog.getByRole('combobox', { name: '模型', exact: true })).toBeVisible()
  await expect(dialog.getByRole('button', { name: '刷新模型', exact: true })).toBeEnabled()
  await page.route('**/models/query', (route) =>
    route.fulfill({
      status: 400,
      contentType: 'application/json',
      body: JSON.stringify({ error: 'discovery failed' }),
    }),
  )
  await dialog.getByRole('button', { name: '刷新模型', exact: true }).click()
  await expect(dialog.getByRole('combobox', { name: '模型', exact: true })).toHaveCount(0)
  await expect(dialog.getByRole('textbox', { name: '模型', exact: true })).toBeVisible()
  await expect(dialog.getByRole('alert')).toContainText('discovery failed')
  await page.unroute('**/models/query')
  await page.route('**/models/query', (route) =>
    route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ version: 1, models: [] }),
    }),
  )
  await dialog.getByRole('button', { name: '刷新模型', exact: true }).click()
  await expect(dialog).toContainText('未返回可选模型')
  await expect(dialog.getByRole('combobox', { name: '模型', exact: true })).toHaveCount(0)
  await page.unroute('**/models/query')
  await selectChoice(page, dialog, 'Agent', 'mock-async')
  await expect(dialog.getByRole('combobox', { name: '模型', exact: true })).toHaveCount(0)
  await expect(dialog.getByRole('button', { name: '刷新模型', exact: true })).toHaveCount(0)
  await expect(dialog.getByRole('textbox', { name: '模型', exact: true })).toBeVisible()
})

test('compact primary fields, pinyin IDs, fixed suffix and immutable ID on rename', async ({
  page,
}) => {
  await login(page)
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '新建任务', exact: true })
  await expect(dialog.getByRole('textbox', { name: '任务 ID', exact: true })).toHaveCount(0)
  await expect(dialog.getByRole('textbox', { name: '指令', exact: true })).toBeVisible()
  await expect(dialog.getByRole('textbox', { name: '推理强度', exact: true })).not.toBeVisible()
  await dialog.getByLabel('任务名称', { exact: true }).fill('每日仓库检查')
  await dialog.getByLabel('任务名称', { exact: true }).fill('Daily Review中文')
  await dialog.getByLabel('指令', { exact: true }).fill('compact form test')
  await selectChoice(page, dialog, 'Runner', 'test-device')
  await selectChoice(page, dialog, 'Agent', 'mock-sync')
  await expect(dialog.getByRole('combobox', { name: '模型', exact: true })).toContainText(
    '先选择工作目录',
  )
  await selectDirectory(page, dialog)
  await expect(dialog.getByRole('combobox', { name: '模型', exact: true })).toBeEnabled()
  const body = dialog.locator('.modal-body')
  expect(await body.evaluate((el) => el.scrollHeight <= el.clientHeight + 1)).toBeTruthy()
  await page.screenshot({ path: 'test-results/wakeit-create-compact.png', fullPage: true })
  await dialog.locator('.advanced-settings:visible > summary').click()
  await expect(dialog.getByText('填写名称后自动生成', { exact: true })).toHaveCount(0)
  await expect(dialog.getByText('任务 ID', { exact: true })).toHaveCount(0)
  await dialog.getByRole('button', { name: '下一步', exact: true }).click()
  await selectChoice(page, dialog, '类型', 'manual')
  await dialog.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(dialog).not.toBeVisible()
  const records = await (await page.request.get('/api/v1/tasks')).json()
  const record = records.find((r: any) => r.config.name === 'Daily Review中文')
  expect(record.id).toMatch(/^Daily-Review-zhong-wen-[a-f0-9]{10}$/)
  expect(record.config.name).toBe('Daily Review中文')
  await page
    .getByRole('button', { name: /Daily Review中文/ })
    .first()
    .click()
  await page.getByRole('button', { name: '编辑', exact: true }).click()
  const edit = page.getByRole('dialog', { name: '编辑任务', exact: true })
  await edit.getByLabel('任务名称', { exact: true }).fill('Renamed task')
  await edit.getByRole('button', { name: '下一步', exact: true }).click()
  await edit.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(edit).not.toBeVisible()
  const renamed = await (await page.request.get(`/api/v1/tasks/${record.id}`)).json()
  expect(renamed.config.name).toBe('Renamed task')
  expect(renamed.id).toBe(record.id)
})

test('global search scopes, filters and server pagination for Tasks and Runs', async ({ page }) => {
  await login(page)
  await expect(page.locator('.list-pane .search, .list-pane .filter')).toHaveCount(0)
  await page.getByRole('button', { name: '搜索与筛选', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '搜索', exact: true })
  await dialog.locator('.search-scopes').getByRole('button', { name: '任务', exact: true }).click()
  await dialog.getByLabel('搜索内容', { exact: true }).fill('Pagination 中文')
  await dialog.getByLabel('任务状态', { exact: true }).selectOption('false')
  await dialog.getByLabel('任务 Runner', { exact: true }).selectOption('test-device')
  await expect(dialog.locator('.search-group-heading')).toContainText('Tasks23')
  await page.screenshot({ path: 'test-results/wakeit-search-light.png', fullPage: true })
  await dialog.getByRole('button', { name: '应用', exact: true }).click()
  const list = page.locator('.list-pane')
  await expect(list.locator('.task-row')).toHaveCount(20)
  const footer = await list.getByRole('navigation', { name: '分页' }).boundingBox()
  expect(footer!.y + footer!.height).toBeLessThanOrEqual(page.viewportSize()!.height)
  await expect(list.getByRole('navigation', { name: '分页' })).toContainText('1 / 2')
  const firstIDs = await list.locator('.task-row .row-title > span:first-child').allTextContents()
  const response = page.waitForResponse(
    (r) => r.url().includes('/api/v1/tasks?') && r.url().includes('page=2'),
  )
  await list.getByRole('button', { name: '下一页', exact: true }).click()
  expect((await (await response).json()).items).toHaveLength(3)
  await expect(list.locator('.task-row')).toHaveCount(3)
  expect(
    (await list.locator('.task-row .row-title > span:first-child').allTextContents()).some((n) =>
      firstIDs.includes(n),
    ),
  ).toBeFalsy()
  await expect(list.getByRole('button', { name: '下一页', exact: true })).toBeDisabled()
  await list.getByRole('button', { name: '上一页', exact: true }).click()
  await expect(list.locator('.task-row')).toHaveCount(20)

  await page.getByRole('button', { name: '搜索与筛选', exact: true }).click()
  await dialog.locator('.search-scopes').getByRole('button', { name: '运行', exact: true }).click()
  await dialog.getByLabel('搜索内容').fill('Pagination')
  await dialog.getByLabel('运行状态').selectOption('succeeded')
  await expect(dialog.locator('.search-group-heading')).toContainText('Runs23')
  await dialog.getByRole('button', { name: '应用', exact: true }).click()
  await expect(page).toHaveURL(/\/runs/)
  await expect(list.locator('.task-row')).toHaveCount(20)
  await list.getByRole('button', { name: '下一页', exact: true }).click()
  await expect(list.locator('.task-row')).toHaveCount(3)
  await expect(list.getByRole('navigation', { name: '分页' })).toContainText('2 / 2')
  await expect(page.getByText('最近 100 条运行记录', { exact: true })).toHaveCount(0)
  await expect(page.getByText('自动更新 · 工作区共享任务', { exact: true })).toHaveCount(0)
})

test('unscoped search, multiple scopes, navigation, reset and role-aware settings', async ({
  page,
}) => {
  await login(page)
  await page.keyboard.press('Meta+k')
  const dialog = page.getByRole('dialog', { name: '搜索', exact: true })
  await expect(dialog).toBeVisible()
  await expect(dialog.locator('.search-group')).toHaveCount(5)
  await expect(dialog.getByLabel('运行状态')).toHaveCount(0)
  await dialog.getByLabel('搜索内容').fill('Pagination')
  await expect(
    dialog.locator('.search-group').filter({ hasText: '任务' }).locator('.search-result'),
  ).toHaveCount(20)
  await dialog
    .locator('.search-scopes')
    .getByRole('button', { name: '设备', exact: true })
    .click()
  await dialog
    .locator('.search-scopes')
    .getByRole('button', { name: 'Agent', exact: true })
    .click()
  await expect(dialog.locator('.search-group')).toHaveCount(2)
  await dialog.getByLabel('搜索内容').fill('test-device')
  await dialog.getByLabel('设备状态').selectOption('online')
  await expect(
    dialog.locator('.search-group').filter({ hasText: '设备' }).locator('.search-result'),
  ).toHaveCount(1)
  await dialog
    .locator('.search-group')
    .filter({ hasText: '设备' })
    .getByRole('button', { name: /test-device/ })
    .click()
  await expect(page).toHaveURL(/\/runners\?runner=/)
  await expect(page.locator('.runner-row.active')).toContainText('test-device')
  await page.getByRole('button', { name: '搜索与筛选' }).click()
  await dialog.getByRole('button', { name: '清除筛选', exact: true }).click()
  await dialog
    .locator('.search-scopes')
    .getByRole('button', { name: '设置', exact: true })
    .click()
  await dialog.getByLabel('搜索内容').fill('token')
  await dialog.getByRole('button', { name: '个人 API Key', exact: true }).click()
  await expect(dialog).not.toBeVisible()
  await expect(page).toHaveURL(/\/settings\?section=tokens/)
  await expect(page.getByRole('heading', { name: /^API Key/ })).toBeVisible()
  await expect(page.getByRole('heading', { name: '成员', exact: true })).not.toBeVisible()
  await page.getByRole('button', { name: '切换主题', exact: true }).click()
  await page.getByRole('button', { name: '搜索与筛选' }).click()
  await dialog.getByRole('button', { name: '清除筛选', exact: true }).click()
  await dialog.locator('.search-scopes').getByRole('button', { name: '运行', exact: true }).click()
  await dialog.getByLabel('搜索内容').fill('Pagination')
  await expect(dialog.locator('.search-result').first()).toBeVisible()
  await expect(dialog.locator('.search-group-heading')).toContainText('Runs23')
  await page.screenshot({ path: 'test-results/wakeit-search-dark.png', fullPage: true })
  await page.keyboard.press('Escape')
  await expect(dialog).not.toBeVisible()
})

test('product copy and status styles stay consistent across all pages', async ({ page }) => {
  await login(page)
  await expect(page).toHaveTitle('Wakeit')
  const unrun = page.locator('.list-pane .status.not_run').first()
  const succeeded = page.locator('.list-pane .status.succeeded').first()
  await expect(unrun).toBeVisible()
  await expect(unrun.locator('i')).toBeVisible()
  await expect(succeeded).toBeVisible()
  const metrics = (el: Element) => {
    const s = getComputedStyle(el)
    return [s.fontSize, s.fontWeight, s.lineHeight, s.display, s.gap]
  }
  expect(await unrun.evaluate(metrics)).toEqual(await succeeded.evaluate(metrics))

  const banned = [
    'Scheduling for agents',
    '自动更新 · 工作区共享任务',
    '最近 100 条运行记录',
    '修改时会检查版本',
    '内置与自定义 Agent 使用同一套',
    '填写名称后自动生成',
    '接入你自己的 Agent',
    '返回列表',
    '适配模板',
  ]
  for (const name of ['任务', '运行', '设备', '设置']) {
    await page.getByRole('link', { name, exact: true }).click()
    await expect(page.getByRole('heading', { name: new RegExp(`^${name}`) }).first()).toBeVisible()
    for (const text of banned) await expect(page.getByText(text, { exact: false })).toHaveCount(0)
  }
  await page.getByRole('link', { name: '任务', exact: true }).click()
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '新建任务', exact: true })
  await expect(dialog.getByText('任务 ID', { exact: true })).toHaveCount(0)
})
