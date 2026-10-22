import { expect, test, type Page } from '@playwright/test'
import { apiPath, fixtureWorkspace, workspacePage } from './workspace'

async function fixture(
  page: Page,
  role: 'admin' | 'operator' | 'viewer',
  demo = false,
  instanceAdmin = false,
) {
  const calls: { path: string; method: string }[] = []
  const workspace = { id: fixtureWorkspace, name: 'Workspace Preview', role, is_demo: demo }
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    const method = route.request().method()
    calls.push({ path, method })
    const send = (json: unknown, status = 200) => route.fulfill({ json, status })
    if (path === '/api/v1/session')
      return send({ id: 'fixture-user', username: '@fixture', role, instance_admin: instanceAdmin })
    if (path === '/api/v1/workspaces')
      return send({ items: demo ? [] : [workspace], demo: demo ? workspace : null })
    if (path === `/api/v1/workspaces/${fixtureWorkspace}`) return send(workspace)
    if (path.endsWith('/leave')) return send({ error: 'cannot remove the last administrator' }, 409)
    const name = apiPath(path)
    if (name === '/meta')
      return send({ version: '0.2.0', public_url: 'https://example.test', install: {} })
    if (name === '/settings')
      return send({ timezone: 'UTC', log_retention_days: 7, run_retention_days: 90 })
    if (name === '/timezones') return send(['UTC', 'Asia/Shanghai'])
    if (name === '/tokens') return send({ items: [], total: 0, page: 1, page_size: 20 })
    if (name === '/moments/days')
      return send({
        timezone: 'UTC',
        today: '2026-10-19',
        total_runs: 0,
        days: [],
        next_before: null,
      })
    return send([])
  })
  return calls
}

for (const [role, label, english] of [
  ['admin', '管理员', 'Admin'],
  ['operator', '读写', 'Read/write'],
  ['viewer', '只读', 'Read-only'],
] as const) {
  test(`${role}: Settings stays visible, workspace actions move there and permission labels follow the language`, async ({
    page,
  }) => {
    const calls = await fixture(page, role)
    await page.goto(workspacePage('/settings?section=workspace'))
    await expect(page.getByRole('link', { name: '设置', exact: true })).toBeVisible()
    await expect(page.locator('.workspace-name-settings')).toHaveCount(0)
    await expect(page.locator('.workspace-settings .workspace-name-row')).toContainText(
      'Workspace Preview',
    )
    await expect(
      page.locator('.workspace-settings').getByRole('button', { name: '创建工作区', exact: true }),
    ).toBeVisible()
    await expect(
      page.locator('.form-card-head').getByRole('button', { name: '创建工作区', exact: true }),
    ).toBeVisible()
    await expect(page.getByRole('button', { name: '退出工作区', exact: true })).toBeVisible()
    await expect(page.getByRole('button', { name: '更改工作区名称', exact: true })).toHaveCount(
      role === 'admin' ? 1 : 0,
    )
    await expect(page.getByRole('combobox', { name: '默认时区', exact: true })).toHaveCount(
      role === 'admin' ? 1 : 0,
    )
    await expect(page.getByRole('button', { name: '成员', exact: true })).toHaveCount(
      role === 'admin' ? 1 : 0,
    )
    if (role !== 'admin') {
      await expect(page.locator('.workspace-settings')).toContainText('UTC')
      await expect(page.locator('.workspace-settings input')).toHaveCount(0)
    }
    await page.getByRole('button', { name: '用户菜单', exact: true }).click()
    const menu = page.getByRole('menu')
    await expect(menu.getByRole('menuitem', { name: '账号管理', exact: true })).toHaveCount(0)
    await expect(menu.locator('.user-menu-label')).toHaveText('@fixture')
    const selected = menu.getByRole('menuitemradio', { name: 'Workspace Preview', exact: true })
    await expect(selected).toBeChecked()
    await expect(selected).toContainText(label)
    await expect(selected.locator('svg')).toHaveCount(1)
    await expect(menu.getByRole('menuitem', { name: '创建工作区', exact: true })).toHaveCount(0)
    await expect(menu.getByRole('menuitem', { name: '退出工作区', exact: true })).toHaveCount(0)
    await page.keyboard.press('Escape')
    await page.getByRole('button', { name: '创建工作区', exact: true }).click()
    const create = page.getByRole('dialog', { name: '创建工作区', exact: true })
    await expect(create).toBeVisible()
    await create.getByRole('button', { name: '取消', exact: true }).click()
    await page.getByRole('button', { name: '退出工作区', exact: true }).click()
    const leave = page.getByRole('dialog', { name: '退出工作区', exact: true })
    await expect(leave).toContainText('Workspace Preview')
    expect(calls.some((call) => call.path.endsWith('/leave'))).toBe(false)
    await leave.getByRole('button', { name: '退出', exact: true }).click()
    await expect(page.getByRole('alert')).toHaveText('必须至少保留一名有效管理员')
    await expect(leave).toBeVisible()
    await leave.getByRole('button', { name: '取消', exact: true }).click()
    await page.getByRole('button', { name: '切换到英文', exact: true }).click()
    await page.getByRole('button', { name: 'User menu', exact: true }).click()
    await expect(
      page.getByRole('menuitemradio', { name: 'Workspace Preview', exact: true }),
    ).toContainText(english)
    expect(calls.filter((call) => call.method === 'PUT')).toHaveLength(0)
    expect(calls.some((call) => /\/(members|archived)$/.test(call.path) && role !== 'admin')).toBe(
      false,
    )
  })
}

test('demo keeps Settings visible without keys, privileged controls or a leave action', async ({
  page,
}) => {
  const calls = await fixture(page, 'viewer', true)
  await page.goto(workspacePage('/settings?section=tokens'))
  await expect(page.getByRole('link', { name: '设置', exact: true })).toBeVisible()
  await expect(page).toHaveURL(/section=workspace/)
  await expect(page.getByRole('button', { name: '创建工作区', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '退出工作区', exact: true })).toHaveCount(0)
  await expect(page.getByRole('button', { name: '更改工作区名称', exact: true })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'API Key', exact: true })).toHaveCount(0)
  await expect(page.locator('.workspace-settings')).toContainText('UTC')
  expect(calls.some((call) => /\/(tokens|members|archived|timezones)$/.test(call.path))).toBe(false)
})

for (const width of [390, 1440]) {
  test(`compact workspace menu at ${width}px with account actions, keyboard selection and internal scrolling`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 844 })
    await fixture(page, 'viewer', false, true)
    // Keep a long list and names confined to the workspace list, not the account actions.
    await page.route('**/api/v1/workspaces', (route) =>
      route.fulfill({
        json: {
          items: Array.from({ length: 30 }, (_, i) => ({
            id:
              i === 0 ? fixtureWorkspace : `22222222-2222-4333-8444-${String(i).padStart(12, '0')}`,
            name:
              i === 0
                ? 'Workspace Preview'
                : 'A workspace with a deliberately long name '.repeat(3) + i,
            role: 'viewer',
            is_demo: false,
          })),
          demo: null,
        },
      }),
    )
    await page.goto(workspacePage('/settings'))
    for (const dark of [false, true]) {
      if (dark) await page.getByRole('button', { name: '切换主题', exact: true }).click()
      const trigger = page.getByRole('button', { name: '用户菜单', exact: true })
      await trigger.focus()
      await trigger.press('Enter')
      const menu = page.getByRole('menu')
      const accounts = menu.getByRole('menuitem', { name: '账号管理', exact: true })
      const signout = menu.getByRole('menuitem', { name: '退出登录', exact: true })
      await expect(accounts.locator('svg[aria-hidden="true"]')).toHaveCount(1)
      await expect(signout.locator('svg[aria-hidden="true"]')).toHaveCount(1)
      await expect(signout).toHaveCSS('color', dark ? 'rgb(248, 81, 73)' : 'rgb(207, 34, 46)')
      const bounds = (await menu.boundingBox())!
      expect(bounds.x).toBeGreaterThanOrEqual(12)
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(width - 12)
      expect(
        await menu
          .locator('.workspace-menu-list')
          .evaluate((el) => el.scrollHeight > el.clientHeight),
      ).toBe(true)
      await page.keyboard.press('End')
      await expect(signout).toBeFocused()
      await expect(signout).toHaveCSS('color', dark ? 'rgb(248, 81, 73)' : 'rgb(207, 34, 46)')
      await page.screenshot({
        path: test.info().outputPath(`workspace-menu-${width}-${dark ? 'dark' : 'light'}.png`),
      })
      await page.keyboard.press('Escape')
      await expect(trigger).toBeFocused()
    }
    await page.getByRole('button', { name: '切换到英文', exact: true }).click()
    await page.getByRole('button', { name: 'User menu', exact: true }).click()
    await expect(
      page.getByRole('menuitem', { name: 'Account management', exact: true }),
    ).toBeVisible()
    await expect(page.getByRole('menuitem', { name: 'Sign out', exact: true })).toHaveCSS(
      'color',
      'rgb(248, 81, 73)',
    )
  })
}
