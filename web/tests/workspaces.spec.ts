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
  await expect(page.getByRole('button', { name: '切换工作区', exact: true })).toBeVisible()
  await page.getByRole('link', { name: '任务', exact: true }).click()
  await expect(page.locator('.list-pane')).toContainText('Workspace a')
  await expect(page.getByRole('button', { name: '新建任务', exact: true })).toBeVisible()

  const other = await context.newPage()
  await other.goto(`/w/${stack.workspaces[1]}/tasks`)
  await expect(other.locator('.list-pane')).toContainText('Workspace b')
  await expect(other.getByRole('button', { name: '新建任务', exact: true })).toHaveCount(0)
  await expect(page.locator('.list-pane')).toContainText('Workspace a')
  await expect(page).toHaveURL(new RegExp(`/w/${stack.workspaces[0]}/tasks`))

  await page.getByRole('button', { name: '切换工作区', exact: true }).click()
  await page.getByRole('menuitem', { name: 'B', exact: true }).click()
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
  await expect(page.getByRole('button', { name: '新建账号', exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '用户菜单', exact: true }).click()
  await page.getByRole('menuitem', { name: '实例账号管理', exact: true }).click()
  await expect(page).toHaveURL('/instance')
  await expect(page.getByRole('button', { name: '新建账号', exact: true })).toBeVisible()
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
