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
const headers = { 'X-Wakeit-Request': '1' }
async function login(page: Page, role = 'admin') {
  await loginSession(page, role, role === 'admin' ? stack.password : 'wakeit-role-test-password')
  await page.getByRole('link', { name: '设置', exact: true }).click()
}
const logInput = (page: Page) => page.getByLabel('日志保留', { exact: true })
async function refocus(page: Page) {
  await page.evaluate(() => {
    Object.defineProperty(document, 'visibilityState', { value: 'hidden', configurable: true })
    window.dispatchEvent(new Event('visibilitychange'))
    Object.defineProperty(document, 'visibilityState', { value: 'visible', configurable: true })
    window.dispatchEvent(new Event('visibilitychange'))
  })
}
test('workspace drafts survive refresh, validate bounds and guard every navigation entry', async ({
  page,
}) => {
  await login(page)
  const original = await (await page.request.get('/api/v1/settings')).json()
  try {
    await expect(page.getByRole('button', { name: '工作区', exact: true })).toHaveAttribute(
      'aria-current',
      'page',
    )
    await expect(page.getByRole('heading', { name: /^成员/ })).toHaveCount(0)
    await expect(page.getByRole('button', { name: '保存', exact: true })).toHaveCount(0)
    await logInput(page).fill('3651')
    await expect(page.getByRole('button', { name: '保存', exact: true })).toBeDisabled()
    await expect(page.getByText('请输入 1–3650 的整数', { exact: true })).toBeVisible()
    await logInput(page).fill(String(original.log_retention_days + 2))
    await page.request.put('/api/v1/settings', {
      data: { ...original, log_retention_days: original.log_retention_days + 1 },
      headers,
    })
    const refresh = page.waitForResponse(
      (r) => r.url().endsWith('/api/v1/settings') && r.request().method() === 'GET',
    )
    await refocus(page)
    await refresh
    await expect(logInput(page)).toHaveValue(String(original.log_retention_days + 2))
    await page.getByRole('button', { name: '成员', exact: true }).click()
    const leave = page.getByRole('dialog', { name: '有未保存的更改', exact: true })
    await expect(leave).toBeVisible()
    await leave.getByRole('button', { name: '继续编辑', exact: true }).click()
    await expect(page.getByRole('button', { name: '工作区' })).toHaveAttribute(
      'aria-current',
      'page',
    )
    await expect(logInput(page)).toHaveValue(String(original.log_retention_days + 2))
    await page.getByRole('button', { name: '取消', exact: true }).click()
    await expect(logInput(page)).toHaveValue(String(original.log_retention_days + 1))
    await logInput(page).fill(String(original.log_retention_days + 3))
    await page.getByRole('link', { name: '任务', exact: true }).click()
    await expect(leave).toBeVisible()
    await leave.getByRole('button', { name: '放弃更改' }).click()
    await expect(page.getByRole('heading', { name: /^任务/ })).toBeVisible()
  } finally {
    await page.request.put('/api/v1/settings', { data: original, headers })
  }
})
test('timezone selection, failed save, search protection and save before leaving', async ({
  page,
}) => {
  await login(page)
  const original = await (await page.request.get('/api/v1/settings')).json()
  try {
    let puts = 0
    await page.route('**/api/v1/settings', (route) => {
      if (route.request().method() === 'PUT') {
        puts++
        return route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ error: '保存失败' }),
        })
      }
      return route.continue()
    })
    await page.getByRole('combobox', { name: '默认时区' }).click()
    await page.getByLabel('搜索时区').fill('UTC')
    await page.getByRole('option', { name: 'UTC', exact: true }).click()
    expect(puts).toBe(0)
    await page.getByRole('button', { name: '保存', exact: true }).click()
    await expect(page.getByText('保存失败', { exact: true })).toBeVisible()
    await expect(page.getByRole('combobox', { name: '默认时区' })).toContainText('UTC')
    await page.getByRole('button', { name: '搜索与筛选' }).click()
    const search = page.getByRole('dialog', { name: '搜索', exact: true })
    await search
      .locator('.search-scopes')
      .getByRole('button', { name: '设置', exact: true })
      .click()
    await search.getByLabel('搜索内容').fill('token')
    await search.getByRole('button', { name: '个人 API Key', exact: true }).click()
    const leave = page.getByRole('dialog', { name: '有未保存的更改', exact: true })
    await expect(leave).toBeVisible()
    await leave.getByRole('button', { name: '继续编辑' }).click()
    await expect(page.getByRole('button', { name: '工作区' })).toHaveAttribute(
      'aria-current',
      'page',
    )
    await page.unroute('**/api/v1/settings')
    await page.getByRole('button', { name: 'API Key' }).click()
    await leave.getByRole('button', { name: '保存并离开' }).click()
    await expect(page.getByRole('heading', { name: /^API Key/ })).toBeVisible()
    expect((await (await page.request.get('/api/v1/settings')).json()).timezone).toBe('UTC')
  } finally {
    await page.unroute('**/api/v1/settings')
    await page.request.put('/api/v1/settings', { data: original, headers })
  }
})
test('member pages, last admin, inactive status and failed role changes', async ({ page }) => {
  await login(page)
  const totalMembers = (await (await page.request.get('/api/v1/users')).json()).length
  await page.getByRole('button', { name: '成员', exact: true }).click()
  await expect(page.locator('.settings-member-name')).toHaveCount(20)
  await page.getByRole('button', { name: '下一页' }).click()
  await expect(page.locator('.settings-member-name')).toHaveCount(Math.min(20, totalMembers - 20))
  await page.getByRole('button', { name: '上一页' }).click()
  await page.getByRole('button', { name: 'admin 角色', exact: true }).click()
  await expect(page.getByRole('menuitemradio', { name: '读写', exact: true })).toBeDisabled()
  await page.keyboard.press('Escape')
  const inactive = page.locator('.settings-member-row').filter({ hasText: 'inactive-fixture' })
  await inactive.getByRole('button', { name: 'inactive-fixture 角色' }).click()
  await page.getByRole('menuitemradio', { name: '读写', exact: true }).click()
  await expect(inactive.getByRole('button', { name: 'inactive-fixture 角色' })).toContainText(
    '读写',
  )
  const members = await (await page.request.get('/api/v1/users')).json()
  const record = members.find((m: any) => m.username === 'inactive-fixture')
  expect(record.active).toBe(false)
  await page.route(`**/api/v1/users/${record.id}`, (route) =>
    route.fulfill({
      status: 500,
      contentType: 'application/json',
      body: JSON.stringify({ error: '角色保存失败' }),
    }),
  )
  await inactive.getByRole('button', { name: 'inactive-fixture 角色' }).click()
  await page.getByRole('menuitemradio', { name: '只读', exact: true }).click()
  await expect(inactive.getByText('角色保存失败', { exact: true })).toBeVisible()
  await expect(inactive.getByRole('button', { name: 'inactive-fixture 角色' })).toContainText(
    '读写',
  )
  await page.unroute(`**/api/v1/users/${record.id}`)
  await inactive.getByRole('button', { name: 'inactive-fixture 角色' }).click()
  await page.getByRole('menuitemradio', { name: '只读', exact: true }).click()
  await expect(inactive.getByRole('button', { name: 'inactive-fixture 角色' })).toContainText(
    '只读',
  )
  await page.screenshot({ path: 'test-results/settings-members-light.png', fullPage: true })
})
test('own downgrade refreshes permissions and invalid sections resolve by role', async ({
  page,
  request,
}) => {
  await login(page)
  const users = await (await page.request.get('/api/v1/users')).json()
  const admin = users.find((u: any) => u.username === 'admin'),
    operator = users.find((u: any) => u.username === 'operator')
  const adminToken = (await page.context().cookies()).find(
    (c) => c.name === 'wakeit_session',
  )!.value
  await page.request.put(`/api/v1/users/${operator.id}`, {
    data: { role: 'admin', active: true },
    headers,
  })
  try {
    await page.getByRole('button', { name: '成员' }).click()
    await page.getByRole('button', { name: 'admin 角色', exact: true }).click()
    await page.getByRole('menuitemradio', { name: '读写', exact: true }).click()
    const dialog = page.getByRole('dialog', { name: '更改自己的角色' })
    await expect(dialog).toBeVisible()
    expect((await (await page.request.get('/api/v1/session')).json()).role).toBe('admin')
    await dialog.getByRole('button', { name: '确认更改' }).click()
    await expect(page.getByRole('button', { name: '工作区', exact: true })).toBeVisible()
    await expect(page.locator('.workspace-settings')).toBeVisible()
    await expect(page.locator('.workspace-settings input')).toHaveCount(0)
    await page.goto('/settings?section=members')
    await expect(page).toHaveURL(/section=workspace/)
    await expect(page.locator('.settings-category')).toHaveCount(3)
    await expect(page.getByRole('button', { name: '归档任务', exact: true })).toBeVisible()
  } finally {
    const session = await (
      await request.post('/api/v1/session', {
        data: { username: 'operator', password: 'wakeit-role-test-password' },
      })
    ).json()
    await request.put(`/api/v1/users/${admin.id}`, {
      data: { role: 'admin', active: true },
      headers: { Authorization: `Bearer ${session.token}` },
    })
    await request.put(`/api/v1/users/${operator.id}`, {
      data: { role: 'operator', active: true },
      headers: { Authorization: `Bearer ${adminToken}` },
    })
    await request.delete('/api/v1/session', {
      headers: { Authorization: `Bearer ${session.token}` },
    })
  }
})
test('API Key pages, duplicate create, one-time reveal, clipboard failure and failed revoke', async ({
  page,
}) => {
  await login(page)
  const totalTokens = (await (await page.request.get('/api/v1/tokens')).json()).length
  await page.getByRole('button', { name: 'API Key' }).click()
  await expect(page.locator('.settings-token-row:not(.settings-table-head)')).toHaveCount(20)
  await page.getByRole('button', { name: '下一页' }).click()
  await expect(page.locator('.settings-token-row:not(.settings-table-head)')).toHaveCount(
    Math.min(20, totalTokens - 20),
  )
  await page.getByRole('button', { name: '上一页' }).click()
  let creates = 0
  await page.route('**/api/v1/tokens', async (route) => {
    if (route.request().method() === 'POST') {
      creates++
      await new Promise((resolve) => setTimeout(resolve, 150))
    }
    await route.continue()
  })
  await page.getByRole('button', { name: '创建 API Key' }).click()
  const dialog = page.getByRole('dialog', { name: '创建 API Key', exact: true })
  const name = `settings-secret-${Date.now()}`
  await dialog.getByLabel('名称').fill(name)
  await dialog.locator('form').evaluate((form) => {
    form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
    form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
  })
  await expect(dialog.getByLabel('API Key', { exact: true })).not.toHaveValue('')
  expect(creates).toBe(1)
  const secret = await dialog.getByLabel('API Key', { exact: true }).inputValue()
  expect(page.url()).not.toContain(secret)
  expect(
    await page.evaluate(
      (secret) => Object.values(localStorage).some((v) => v.includes(secret)),
      secret,
    ),
  ).toBeFalsy()
  await page.evaluate(() => {
    navigator.clipboard.writeText = async () => {
      throw new Error('blocked')
    }
  })
  await dialog.getByRole('button', { name: '复制API Key', exact: true }).click()
  await expect(dialog.getByText('复制失败，请手动复制', { exact: true })).toBeVisible()
  await dialog.getByRole('button', { name: '关闭', exact: true }).last().click()
  expect(
    await page
      .locator('input')
      .evaluateAll(
        (inputs, secret) => inputs.some((input) => (input as HTMLInputElement).value === secret),
        secret,
      ),
  ).toBeFalsy()
  const row = page.locator('.settings-token-row').filter({ hasText: name })
  const records = await (await page.request.get('/api/v1/tokens')).json()
  const record = records.find((t: any) => t.name === name)
  await page.route(`**/api/v1/tokens/${record.id}`, (route) =>
    route.fulfill({
      status: 500,
      contentType: 'application/json',
      body: JSON.stringify({ error: '撤销失败' }),
    }),
  )
  await row.getByRole('button', { name: '撤销' }).click()
  const revoke = page.getByRole('dialog', { name: '撤销 API Key' })
  await revoke.getByRole('button', { name: '撤销', exact: true }).click()
  await expect(revoke.getByText('撤销失败', { exact: true })).toBeVisible()
  await expect(row).toBeVisible()
  await page.unroute(`**/api/v1/tokens/${record.id}`)
  await revoke.getByRole('button', { name: '撤销', exact: true }).click()
  await expect(row).toHaveCount(0)
  await page.getByRole('button', { name: '创建 API Key' }).click()
  await expect(dialog.getByLabel('API Key', { exact: true })).toHaveCount(0)
})
test('API Key expiry options, custom validation and settings headings', async ({ page }) => {
  await login(page)
  await expect(page.locator('.detail-heading .count')).toHaveCount(0)
  await page.getByRole('button', { name: '成员', exact: true }).click()
  await expect(page.getByRole('heading', { name: '成员', exact: true })).toBeVisible()
  await expect(page.locator('.detail-heading .count')).toHaveCount(0)
  await page.getByRole('button', { name: 'API Key', exact: true }).click()
  await expect(page.getByRole('heading', { name: 'API Key', exact: true })).toBeVisible()
  await expect(page.locator('.detail-heading .count')).toHaveCount(0)
  const create = page.getByRole('button', { name: '创建 API Key', exact: true })
  await expect(create.locator('.lucide-plus')).toBeVisible()
  const dialog = page.getByRole('dialog', { name: '创建 API Key', exact: true })
  for (const choice of ['不过期', '7 天', '自定义时间']) {
    await create.click()
    await expect(dialog.getByRole('combobox', { name: '过期时间', exact: true })).toHaveText(
      '不过期',
    )
    const name = `expiry-${choice}-${Date.now()}`
    await dialog.getByLabel('名称', { exact: true }).fill(name)
    const custom = new Date(Date.now() + 2 * 86400000)
    custom.setSeconds(0, 0)
    if (choice !== '不过期') {
      await dialog.getByRole('combobox', { name: '过期时间', exact: true }).click()
      await page.getByRole('option', { name: choice, exact: true }).click()
    }
    if (choice === '自定义时间') {
      await dialog.getByLabel('到期时间', { exact: true }).fill('2000-01-01T12:00')
      await dialog.getByRole('button', { name: '创建', exact: true }).click()
      await expect(dialog.getByRole('alert')).toHaveText('请选择未来的过期时间')
      const local = new Date(custom.getTime() - custom.getTimezoneOffset() * 60000)
        .toISOString()
        .slice(0, 16)
      await dialog.getByLabel('到期时间', { exact: true }).fill(local)
      await page.route('**/api/v1/tokens', (route) =>
        route.request().method() === 'POST'
          ? route.fulfill({
              status: 500,
              contentType: 'application/json',
              body: JSON.stringify({ error: '创建失败' }),
            })
          : route.continue(),
      )
      await dialog.getByRole('button', { name: '创建', exact: true }).click()
      await expect(dialog.getByRole('alert')).toHaveText('创建失败')
      await expect(dialog.getByLabel('到期时间', { exact: true })).toHaveValue(local)
      await page.unroute('**/api/v1/tokens')
    }
    await dialog.getByRole('button', { name: '创建', exact: true }).click()
    await expect(dialog.getByLabel('API Key', { exact: true })).not.toHaveValue('')
    await dialog.getByRole('button', { name: '关闭', exact: true }).last().click()
    const records = await (await page.request.get('/api/v1/tokens')).json()
    const key = records.find((key: any) => key.name === name)
    try {
      const row = page.locator('.settings-token-row').filter({ hasText: name })
      if (choice === '不过期') {
        expect(key.expires_at).toBeNull()
        await expect(row.getByText('不过期', { exact: true })).toBeVisible()
        await expect(row.getByText('已过期', { exact: true })).toHaveCount(0)
      } else if (choice === '7 天') {
        expect(
          Math.abs(new Date(key.expires_at).getTime() - Date.now() - 7 * 86400000),
        ).toBeLessThan(30000)
      } else expect(new Date(key.expires_at).getTime()).toBe(custom.getTime())
    } finally {
      await page.request.delete(`/api/v1/tokens/${key.id}`, { headers })
    }
  }
})
test('settings layouts and independent loading failures across roles', async ({ page }) => {
  await login(page)
  await expect(page.getByRole('combobox', { name: '默认时区' })).toBeVisible()
  await page.screenshot({ path: 'test-results/settings-workspace-light.png', fullPage: true })
  await page.getByRole('button', { name: '切换主题' }).click()
  await page.screenshot({ path: 'test-results/settings-workspace-dark.png', fullPage: true })
  await page.setViewportSize({ width: 2200, height: 1100 })
  const panel = await page.locator('.detail-content.limited').boundingBox()
  const main = await page.locator('.detail-scroll').boundingBox()
  expect(panel!.width).toBeCloseTo(main!.width - 36, 0)
  expect(panel!.x).toBe(main!.x + 18)
  await expect(page.locator('.topbar .breadcrumbs')).toContainText('工作区')
  await expect(page.locator('.detail-pane > .detail-heading')).toHaveCount(0)
  await page.getByRole('button', { name: '工作区', exact: true }).click()
  await page.setViewportSize({ width: 390, height: 840 })
  await page
    .getByRole('navigation', { name: '层级导航' })
    .getByRole('button', { name: '设置', exact: true })
    .click()
  await expect(page.getByRole('button', { name: 'API Key' })).toBeVisible()
  await page.getByRole('button', { name: '工作区', exact: true }).click()
  await expect(page.getByRole('combobox', { name: '默认时区' })).toBeVisible()
  await page.screenshot({ path: 'test-results/settings-workspace-mobile.png', fullPage: true })
  await page
    .getByRole('navigation', { name: '层级导航' })
    .getByRole('button', { name: '设置', exact: true })
    .click()
  await page.getByRole('button', { name: 'API Key' }).click()
  await page.getByRole('button', { name: '页面操作', exact: true }).click()
  await expect(page.getByRole('button', { name: '创建 API Key' })).toBeVisible()
  await page.keyboard.press('Escape')
  await page.screenshot({ path: 'test-results/settings-tokens-mobile.png', fullPage: true })
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.screenshot({ path: 'test-results/settings-tokens-dark.png', fullPage: true })
  await page.route('**/api/v1/users?*', (route) =>
    route.fulfill({
      status: 500,
      contentType: 'application/json',
      body: JSON.stringify({ error: '成员加载失败' }),
    }),
  )
  await page.getByRole('button', { name: '成员' }).click()
  await expect(page.getByText('成员加载失败', { exact: true })).toBeVisible()
  await page.getByRole('button', { name: '工作区' }).click()
  await expect(page.getByRole('combobox', { name: '默认时区' })).toBeVisible()
})
