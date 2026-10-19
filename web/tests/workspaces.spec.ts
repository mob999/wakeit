import { test, expect } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'

const stack = JSON.parse(
  fs.readFileSync(
    process.env.WAKEIT_TEST_STACK ?? path.resolve('../.local/integration-stack.json'),
    'utf8',
  ),
)
test.skip(!stack.workspaces, 'Requires the isolated workspace acceptance stack')

async function enterDefault(page: import('@playwright/test').Page) {
  await page.goto('/')
  await page.getByLabel('用户名', { exact: true }).fill('admin')
  await page.getByLabel('密码', { exact: true }).fill('workspace-fixture-password')
  await page.getByRole('button', { name: '登录工作区', exact: true }).click()
  await page.getByRole('link', { name: /Default/ }).click()
  await expect(page.getByRole('button', { name: '用户菜单', exact: true })).toBeVisible()
  await expect(page.locator('.user-menu .avatar')).toHaveCount(0)
  await expect(page.locator('#primary-navigation .workspace-switcher')).toHaveCount(0)
}

test('workspace switch guards unsaved edits and starts the other workspace with fresh state', async ({
  page,
}) => {
  await enterDefault(page)
  await page.getByRole('link', { name: '设置', exact: true }).click()
  const input = page.getByLabel('日志保留', { exact: true })
  await expect(input).toBeVisible()
  const original = await input.inputValue()
  await input.fill(String(Number(original) + 1))
  for (const action of ['创建工作区', '退出工作区']) {
    await page.getByRole('button', { name: action, exact: true }).click()
    const prompt = page.getByRole('dialog', { name: '有未保存的更改', exact: true })
    await expect(prompt).toBeVisible()
    await prompt.getByRole('button', { name: '继续编辑', exact: true }).click()
    await expect(input).toHaveValue(String(Number(original) + 1))
    await expect(page.getByRole('dialog', { name: action, exact: true })).toHaveCount(0)
  }
  await page.getByRole('button', { name: '用户菜单', exact: true }).click()
  await page.getByRole('menuitemradio', { name: 'B', exact: true }).click()
  const guard = page.getByRole('dialog', { name: '有未保存的更改', exact: true })
  await expect(guard).toBeVisible()
  await guard.getByRole('button', { name: '继续编辑', exact: true }).click()
  await expect(input).toHaveValue(String(Number(original) + 1))
  await expect(page).toHaveURL(new RegExp(`/w/${stack.workspaces[0]}/settings`))
  await page.getByRole('button', { name: '用户菜单', exact: true }).click()
  await page.getByRole('menuitemradio', { name: 'B', exact: true }).click()
  await guard.getByRole('button', { name: '放弃更改', exact: true }).click()
  await expect(page).toHaveURL(new RegExp(`/w/${stack.workspaces[1]}/`))
  await expect(page.getByRole('link', { name: '设置', exact: true })).toHaveCount(1)
  const saved = await (
    await page.request.get(`/api/v1/workspaces/${stack.workspaces[0]}/settings`)
  ).json()
  expect(String(saved.log_retention_days)).toBe(original)
})

test('create and rename a workspace without inheriting tasks or roles from another workspace', async ({
  page,
}) => {
  await enterDefault(page)
  await page.getByRole('link', { name: '设置', exact: true }).click()
  await page.getByRole('button', { name: '创建工作区', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '创建工作区', exact: true })
  const name = `Browser fixture ${Date.now()}`
  await dialog.getByLabel('工作区名称', { exact: true }).fill(name)
  await dialog.getByRole('button', { name: '创建', exact: true }).click()
  await expect(page).not.toHaveURL(new RegExp(`/w/${stack.workspaces[0]}/`))
  await expect(page).toHaveURL(/\/w\/[0-9a-f-]+\/$/)
  expect(page.url()).not.toContain(stack.workspaces[0])
  await page.getByRole('button', { name: '用户菜单', exact: true }).click()
  await expect(page.getByRole('menuitemradio', { name, exact: true })).toBeVisible()
  await page.keyboard.press('Escape')
  await page.getByRole('link', { name: '任务', exact: true }).click()
  await expect(page.getByRole('button', { name: '新建任务', exact: true }).first()).toBeVisible()
  await expect(page.locator('.list-pane')).not.toContainText('Workspace a')
  await page.getByRole('link', { name: '设置', exact: true }).click()
  await page.getByRole('button', { name: '更改工作区名称', exact: true }).click()
  const rename = page.getByRole('dialog', { name: '更改工作区名称', exact: true })
  await rename.getByLabel('工作区名称', { exact: true }).fill(name + ' renamed')
  await rename.getByRole('button', { name: '保存', exact: true }).click()
  await expect(rename).not.toBeVisible()
  await page.getByRole('button', { name: '用户菜单', exact: true }).click()
  await expect(
    page.getByRole('menuitemradio', { name: name + ' renamed', exact: true }),
  ).toBeVisible()
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: '成员', exact: true }).click()
  await expect(page.locator('.settings-member-name').filter({ hasText: /^admin$/ })).toHaveText(
    'admin',
  )
  await expect(page.locator('.settings-member-name')).toHaveCount(1)
})

test('workspace URLs, independent tabs, role-specific controls and scoped requests', async ({
  page,
  context,
}) => {
  await page.goto('/')
  await page.getByLabel('用户名', { exact: true }).fill('admin')
  await page.getByLabel('密码', { exact: true }).fill('workspace-fixture-password')
  await page.getByRole('button', { name: '登录工作区' }).click()
  await expect(page.getByRole('heading', { name: '工作区', exact: true })).toBeVisible()
  await page.getByRole('link', { name: /Default/ }).click()
  await expect(page).toHaveURL(new RegExp(`/w/${stack.workspaces[0]}/`))
  await expect(page.getByRole('button', { name: '用户菜单', exact: true })).toBeVisible()
  await expect(page.locator('.user-menu .avatar')).toHaveCount(0)
  await expect(page.locator('#primary-navigation .workspace-switcher')).toHaveCount(0)
  await page.getByRole('link', { name: '任务', exact: true }).click()
  await expect(page.locator('.list-pane')).toContainText('Workspace a')
  await expect(page.getByRole('button', { name: '新建任务', exact: true })).toBeVisible()

  const other = await context.newPage()
  await other.goto(`/w/${stack.workspaces[1]}/tasks`)
  await expect(other.locator('.list-pane')).toContainText('Workspace b')
  await expect(other.getByRole('button', { name: '新建任务', exact: true })).toHaveCount(0)
  await expect(page.locator('.list-pane')).toContainText('Workspace a')
  await expect(page).toHaveURL(new RegExp(`/w/${stack.workspaces[0]}/tasks`))

  await page.getByRole('button', { name: '用户菜单', exact: true }).click()
  await page.getByRole('menuitemradio', { name: 'B', exact: true }).click()
  await expect(page).toHaveURL(new RegExp(`/w/${stack.workspaces[1]}/`))
  await page.getByRole('link', { name: '任务', exact: true }).click()
  await expect(page.locator('.list-pane')).toContainText('Workspace b')
  await expect(page.locator('.list-pane')).not.toContainText('Workspace a')
})

test('workspace and instance accounts remain separate and responsive in both themes', async ({
  page,
}) => {
  await page.goto('/')
  await page.getByLabel('用户名', { exact: true }).fill('admin')
  await page.getByLabel('密码', { exact: true }).fill('workspace-fixture-password')
  await page.getByRole('button', { name: '登录工作区' }).click()
  await page.getByRole('link', { name: /Default/ }).click()
  await page.getByRole('link', { name: '设置', exact: true }).click()
  await page.getByRole('button', { name: '成员', exact: true }).click()
  await expect(page.getByRole('button', { name: '邀请成员', exact: true })).toBeVisible()
  await expect(page.locator('.settings-member-name').filter({ hasText: /^admin$/ })).toHaveText(
    'admin',
  )
  await expect(page.locator('.avatar')).toHaveCount(0)
  await expect(page.getByRole('button', { name: '新建账号', exact: true })).toHaveCount(0)
  for (const width of [390, 1440, 2200]) {
    await page.setViewportSize({ width, height: 900 })
    for (const theme of ['light', 'dark']) {
      await page.evaluate((theme) => {
        document.documentElement.dataset.theme = theme
      }, theme)
      const account = page.getByRole('button', { name: '用户菜单', exact: true })
      await expect(account).toBeVisible()
      await expect(account.locator('.user-name')).toHaveText('admin')
      await account.click()
      const menu = page.getByRole('menu')
      await expect(menu.getByRole('menuitemradio', { name: 'Default', exact: true })).toBeVisible()
      await expect(
        menu
          .getByRole('menuitemradio', { name: 'Default', exact: true })
          .locator('.workspace-check'),
      ).toHaveCount(1)
      const bounds = (await menu.boundingBox())!
      expect(bounds.x).toBeGreaterThanOrEqual(0)
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(width)
      await page.screenshot({ path: test.info().outputPath(`account-menu-${width}-${theme}.png`) })
      await page.keyboard.press('Escape')
      await expect(account).toBeFocused()
    }
  }
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.getByRole('button', { name: '用户菜单', exact: true }).click()
  await page.getByRole('menuitem', { name: '实例账号管理', exact: true }).click()
  await expect(page).toHaveURL('/instance')
  await expect(page.getByRole('button', { name: '新建账号', exact: true })).toBeVisible()
  await expect(page.locator('.settings-member-name').filter({ hasText: /^admin$/ })).toHaveText(
    'admin',
  )
  await expect(page.locator('.avatar')).toHaveCount(0)
  await expect(page.getByRole('button', { name: '邀请成员', exact: true })).toHaveCount(0)
  for (const width of [390, 1440, 2200]) {
    await page.setViewportSize({ width, height: 900 })
    for (const theme of ['light', 'dark']) {
      await page.evaluate((theme) => {
        localStorage.setItem('wakeit-theme', theme)
      }, theme)
      await page.reload()
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
        true,
      )
      await expect(page.locator('.content-card').first()).toBeVisible()
      await page.screenshot({ path: test.info().outputPath(`instance-${width}-${theme}.png`) })
    }
  }
})

test('a read-only member leaves the current workspace only after confirming in Settings', async ({
  page,
}) => {
  await enterDefault(page)
  await page.getByRole('button', { name: '用户菜单', exact: true }).click()
  await page.getByRole('menuitemradio', { name: 'B', exact: true }).click()
  await page.getByRole('link', { name: '设置', exact: true }).click()
  await expect(page.getByRole('button', { name: '更改工作区名称', exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '退出工作区', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '退出工作区', exact: true })
  await dialog.getByRole('button', { name: '取消', exact: true }).click()
  let memberships = await (await page.request.get('/api/v1/workspaces')).json()
  expect(memberships.items.some((item: { id: string }) => item.id === stack.workspaces[1])).toBe(
    true,
  )
  await page.getByRole('button', { name: '退出工作区', exact: true }).click()
  await dialog.getByRole('button', { name: '退出', exact: true }).click()
  await expect(page).not.toHaveURL(new RegExp(`/w/${stack.workspaces[1]}/`))
  memberships = await (await page.request.get('/api/v1/workspaces')).json()
  expect(memberships.items.some((item: { id: string }) => item.id === stack.workspaces[1])).toBe(
    false,
  )
  expect(memberships.items.some((item: { id: string }) => item.id === stack.workspaces[0])).toBe(
    true,
  )
})
