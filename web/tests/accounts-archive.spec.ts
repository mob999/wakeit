import { test, expect, type Page } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
import { randomUUID } from 'node:crypto'
import { loginSession } from './session'
const stack = JSON.parse(
  fs.readFileSync(
    process.env.WAKEIT_TEST_STACK ?? path.resolve('../.local/integration-stack.json'),
    'utf8',
  ),
)
const headers = { 'X-Wakeit-Request': '1', 'Idempotency-Key': randomUUID() }
const password = 'wakeit-browser-account-password'
async function login(page: Page, role = 'admin') {
  await loginSession(page, role, role === 'admin' ? stack.password : 'wakeit-role-test-password')
}
async function members(page: Page) {
  await page.getByRole('link', { name: '设置', exact: true }).click()
  await page.getByRole('button', { name: '成员', exact: true }).click()
}
async function memberRow(page: Page, name: string) {
  const row = page
    .locator('.settings-member-row')
    .filter({ has: page.getByRole('button', { name: `${name} 角色`, exact: true }) })
  if (
    !(await row.count()) &&
    (await page.getByRole('button', { name: '下一页', exact: true }).isEnabled())
  )
    await page.getByRole('button', { name: '下一页', exact: true }).click()
  await expect(row).toBeVisible()
  return row
}
test('members create with guarded submission, reset passwords, disable and enable without reviving keys', async ({
  page,
  request,
}) => {
  await login(page)
  await members(page)
  await page.getByRole('button', { name: '新建账号', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '新建账号', exact: true })
  const username = `browser-account-${Date.now()}`
  await dialog.getByLabel('用户名', { exact: true }).fill(username)
  await dialog.getByLabel('密码', { exact: true }).fill(password)
  await dialog.getByLabel('确认密码', { exact: true }).fill('different-password')
  await dialog.getByRole('button', { name: '创建', exact: true }).click()
  await expect(page.getByRole('alert')).toHaveText('两次输入的密码不一致')
  await dialog.getByLabel('确认密码', { exact: true }).fill(password)
  await expect(dialog.getByRole('combobox', { name: '角色', exact: true })).toHaveText('读写')
  await page.route('**/api/v1/users', (route) =>
    route.request().method() === 'POST'
      ? route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ error: '创建失败' }),
        })
      : route.continue(),
  )
  await dialog.getByRole('button', { name: '创建', exact: true }).click()
  await expect(page.getByRole('alert')).toHaveText('创建失败')
  await expect(dialog.getByLabel('用户名', { exact: true })).toHaveValue(username)
  await page.unroute('**/api/v1/users')
  let creates = 0
  await page.route('**/api/v1/users', async (route) => {
    if (route.request().method() === 'POST') {
      creates++
      await new Promise((r) => setTimeout(r, 120))
    }
    await route.continue()
  })
  await dialog.locator('form').evaluate((form) => {
    form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
    form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
  })
  await expect(dialog).toHaveCount(0)
  expect(creates).toBe(1)
  const row = await memberRow(page, username)
  await expect(row.getByText('启用', { exact: true })).toBeVisible()
  const record = (await (await page.request.get('/api/v1/users')).json()).find(
    (u: any) => u.username === username,
  )
  const session = await (
    await request.post('/api/v1/session', { data: { username, password } })
  ).json()
  const key = await (
    await request.post('/api/v1/tokens', {
      headers: { Authorization: `Bearer ${session.token}` },
      data: { name: 'browser-account-key' },
    })
  ).json()
  await row.getByRole('button', { name: `${username} 操作`, exact: true }).click()
  await page.getByRole('menuitem', { name: '重置密码', exact: true }).click()
  const reset = page.getByRole('dialog', { name: '重置密码', exact: true })
  const changed = password + '-new'
  await reset.getByLabel('新密码', { exact: true }).fill(changed)
  await reset.getByLabel('确认密码', { exact: true }).fill(changed)
  await reset.getByRole('button', { name: '重置密码', exact: true }).click()
  await expect(reset).toHaveCount(0)
  for (const token of [session.token, key.token])
    expect(
      (
        await request.get('/api/v1/session', { headers: { Authorization: `Bearer ${token}` } })
      ).status(),
    ).toBe(401)
  expect((await request.post('/api/v1/session', { data: { username, password } })).status()).toBe(
    401,
  )
  const newer = await (
    await request.post('/api/v1/session', { data: { username, password: changed } })
  ).json()
  await row.getByRole('button', { name: `${username} 操作`, exact: true }).click()
  await page.getByRole('menuitem', { name: '停用账号', exact: true }).click()
  await page
    .getByRole('dialog', { name: '停用账号', exact: true })
    .getByRole('button', { name: '停用', exact: true })
    .click()
  await expect(row.getByText('已停用', { exact: true })).toBeVisible()
  expect(
    (
      await request.get('/api/v1/session', { headers: { Authorization: `Bearer ${newer.token}` } })
    ).status(),
  ).toBe(401)
  await row.getByRole('button', { name: `${username} 操作`, exact: true }).click()
  await page.getByRole('menuitem', { name: '启用账号', exact: true }).click()
  await page
    .getByRole('dialog', { name: '启用账号', exact: true })
    .getByRole('button', { name: '启用', exact: true })
    .click()
  await expect(row.getByText('启用', { exact: true })).toBeVisible()
  expect(
    (
      await request.get('/api/v1/session', { headers: { Authorization: `Bearer ${newer.token}` } })
    ).status(),
  ).toBe(401)
  expect(
    (await request.post('/api/v1/session', { data: { username, password: changed } })).ok(),
  ).toBeTruthy()
  await page.getByRole('button', { name: '新建账号', exact: true }).click()
  await expect(dialog.getByLabel('密码', { exact: true })).toHaveValue('')
  await dialog.getByLabel('用户名', { exact: true }).fill(username)
  await dialog.getByLabel('密码', { exact: true }).fill(password)
  await dialog.getByLabel('确认密码', { exact: true }).fill(password)
  await dialog.getByRole('button', { name: '创建', exact: true }).click()
  await expect(page.getByRole('alert')).toHaveText('username unavailable')
  await dialog.getByRole('button', { name: '取消', exact: true }).click()
  expect((await page.request.get(`/api/v1/users`)).ok()).toBeTruthy()
  await page.screenshot({ path: 'test-results/members-management-light.png', fullPage: true })
  await page.setViewportSize({ width: 390, height: 840 })
  await page.screenshot({ path: 'test-results/members-management-mobile.png', fullPage: true })
  await page.request.put(`/api/v1/users/${record.id}`, { headers, data: { active: false } })
})
test('self password reset logs out and last administrator cannot be disabled', async ({
  page,
  browser,
}) => {
  await login(page)
  await members(page)
  await page.getByRole('button', { name: 'admin 操作', exact: true }).click()
  await expect(page.getByRole('menuitem', { name: '停用账号', exact: true })).toBeDisabled()
  await page.keyboard.press('Escape')
  const username = `browser-self-admin-${Date.now()}`
  const member = await (
    await page.request.post('/api/v1/users', {
      headers,
      data: { username, password, role: 'admin' },
    })
  ).json()
  const context = await browser.newContext()
  try {
    const other = await context.newPage()
    await loginSession(other, username, password)
    await members(other)
    const row = await memberRow(other, username)
    await row.getByRole('button', { name: `${username} 操作`, exact: true }).click()
    await other.getByRole('menuitem', { name: '重置密码', exact: true }).click()
    const modal = other.getByRole('dialog', { name: '重置密码', exact: true })
    await expect(modal.getByText(/你将退出登录/)).toBeVisible()
    await modal.getByLabel('新密码', { exact: true }).fill(password + '-new')
    await modal.getByLabel('确认密码', { exact: true }).fill(password + '-new')
    await modal.getByRole('button', { name: '重置密码', exact: true }).click()
    await expect(other.getByRole('button', { name: '登录工作区', exact: true })).toBeVisible()
    await expect(other.getByRole('link', { name: '设置', exact: true })).toHaveCount(0)
  } finally {
    await context.close()
    await page.request.put(`/api/v1/users/${member.id}`, { headers, data: { active: false } })
  }
})
test('archive and restore preserves history, exposes read-only tasks and respects role access', async ({
  page,
}) => {
  await login(page)
  const fixture = (await (await page.request.get('/api/v1/tasks')).json()).find(
    (t: any) => t.config.agent === 'mock-sync' && t.config.runner === 'test-device',
  )
  const id = `browser-archive-${Date.now()}`
  const name = `浏览器归档任务 ${id}`
  const config = {
    ...fixture.config,
    id,
    name,
    prompt: JSON.stringify({ sleep: 1, text: 'archived browser result' }),
    schedule: { every: '1h', timezone: 'UTC' },
    enabled: true,
  }
  expect(
    (
      await page.request.put(`/api/v1/tasks/${id}`, {
        headers,
        data: { config, expected_revision: 0 },
      })
    ).ok(),
  ).toBeTruthy()
  const run = await (await page.request.post(`/api/v1/tasks/${id}/runs`, { headers })).json()
  await page.goto(`/tasks?task=${id}`)
  await expect(page.getByRole('heading', { name, exact: true })).toBeVisible()
  await page.getByRole('button', { name: '归档任务', exact: true }).click()
  const modal = page.getByRole('dialog', { name: '归档任务', exact: true })
  await modal.getByRole('button', { name: '归档', exact: true }).click()
  await expect(modal).toHaveCount(0)
  await expect
    .poll(async () => (await (await page.request.get(`/api/v1/tasks/${id}`)).json()).archived_at)
    .toBeTruthy()
  await page.goto(`/tasks?task=${id}`)
  await expect(page.getByRole('heading', { name, exact: true })).toBeVisible()
  await expect(
    page.locator('.header-page-actions').getByText('已归档', { exact: true }),
  ).toBeVisible()
  for (const action of ['编辑', '归档任务', '立即执行', '恢复任务'])
    await expect(page.getByRole('button', { name: action, exact: true })).toHaveCount(0)
  await expect
    .poll(async () => (await (await page.request.get(`/api/v1/runs/${run.id}`)).json()).state)
    .toBe('succeeded')
  await page.goto(`/runs?run=${run.id}`)
  await expect(page.getByRole('heading', { name, exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '重试', exact: true })).toHaveCount(0)
  await page.getByRole('link', { name: '设置', exact: true }).click()
  await page.getByRole('button', { name: '归档任务', exact: true }).click()
  const row = page.locator('.archive-row').filter({ hasText: name })
  await expect(row).toBeVisible()
  await expect(page.locator('.archive-row:not(.table-head)')).toHaveCount(20)
  await page.getByRole('button', { name: '下一页', exact: true }).click()
  await expect(page.locator('.archive-row:not(.table-head)')).not.toHaveCount(20)
  await page.getByRole('button', { name: '上一页', exact: true }).click()
  await row.getByRole('button', { name: `恢复 ${name}`, exact: true }).click()
  const restore = page.getByRole('dialog', { name: '恢复任务', exact: true })
  await expect(restore.getByText(/保持暂停/)).toBeVisible()
  await restore.getByRole('button', { name: '恢复', exact: true }).click()
  await expect(row).toHaveCount(0)
  const result = await (await page.request.get(`/api/v1/tasks/${id}`)).json()
  expect(result.archived_at).toBeNull()
  expect(result.config.enabled).toBeFalsy()
  expect(result.next_at).toBeNull()
  expect(result.config.schedule).toMatchObject(config.schedule)
  await page.goto(`/tasks?task=${id}`)
  await expect(page.getByRole('button', { name: '恢复任务', exact: true })).toBeVisible()
  // Keep the shared fixture list clean, without deleting its Run history.
  await page.request.post(`/api/v1/tasks/${id}/archive`, {
    headers,
    data: { expected_revision: result.revision },
  })
  await page.context().clearCookies()
  await loginSession(page, 'operator', 'wakeit-role-test-password')
  await page.goto('/settings?section=archived')
  await expect(page.getByRole('heading', { name: '归档任务', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '成员', exact: true })).toHaveCount(0)
  await page.context().clearCookies()
  await loginSession(page, 'viewer', 'wakeit-role-test-password')
  await page.goto('/settings?section=archived')
  await expect(page.locator('.workspace-settings')).toBeVisible()
  await expect(page.getByRole('button', { name: '创建工作区', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '归档任务', exact: true })).toHaveCount(0)
})
test('archive conflicts and restore failures keep dialogs, settings search finds archived tasks', async ({
  page,
}) => {
  await login(page)
  await page.getByRole('button', { name: '搜索与筛选', exact: true }).click()
  const search = page.getByRole('dialog', { name: '搜索', exact: true })
  await search.getByPlaceholder('搜索…').fill('归档')
  await search.getByRole('button', { name: '归档任务', exact: true }).click()
  await expect(page.getByRole('heading', { name: '归档任务', exact: true })).toBeVisible()
  const row = page.locator('.archive-row:not(.table-head)').first()
  const name = await row.locator('.archive-name').innerText()
  await row.getByRole('button', { name: `恢复 ${name}`, exact: true }).click()
  await page.route('**/api/v1/tasks/*/restore', (route) =>
    route.fulfill({
      status: 409,
      contentType: 'application/json',
      body: JSON.stringify({ error: 'task changed; reload before saving' }),
    }),
  )
  const restore = page.getByRole('dialog', { name: '恢复任务', exact: true })
  await restore.getByRole('button', { name: '恢复', exact: true }).click()
  await expect(page.getByRole('alert')).toHaveText('task changed; reload before saving')
  await restore.getByRole('button', { name: '取消', exact: true }).click()
  await row.locator('.archive-name').click()
  await expect(
    page.locator('.header-page-actions').getByText('已归档', { exact: true }),
  ).toBeVisible()
})
