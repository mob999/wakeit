import { workspacePage, apiPath, mockWorkspace, fixtureWorkspace } from './workspace'
import { test, expect, type Page } from '@playwright/test'
const meta = {
  version: '0.2.0',
  public_url: 'https://wakeit.example.test',
  install: {
    version: '0.2.0',
    release: 'https://github.com/mob999/wakeit/releases/tag/v0.2.0',
    manifest: 'https://github.com/mob999/wakeit/releases/download/v0.2.0/wakeit-release.json',
    checksums: 'https://github.com/mob999/wakeit/releases/download/v0.2.0/SHA256SUMS',
    base_url: 'https://github.com/mob999/wakeit/releases/download/v0.2.0',
  },
}
async function fixture(page: Page, role = 'operator', loggedIn = true) {
  const calls: string[] = []
  await page.route('**/api/v1/**', async (route) => {
    if (await mockWorkspace(route, role)) return
    const path = apiPath(new URL(route.request().url()).pathname)
    if (path === '/session') {
      if (route.request().method() === 'POST') {
        loggedIn = true
        return route.fulfill({ json: { ok: true } })
      }
      return route.fulfill({
        status: loggedIn ? 200 : 401,
        json: loggedIn ? { id: 'user', username: 'tester', role } : { error: 'login required' },
      })
    }
    if (path === '/meta') return route.fulfill({ json: meta })
    if (path === '/moments/days')
      return route.fulfill({
        json: {
          timezone: 'Asia/Shanghai',
          today: '2026-10-15',
          total_runs: 0,
          days: [],
          next_cursor: null,
        },
      })
    if (path.startsWith('/device-authorizations/')) {
      if (route.request().method() === 'POST') {
        calls.push(path)
        return route.fulfill({ json: { state: path.endsWith('/approve') ? 'approved' : 'denied' } })
      }
      return route.fulfill({
        json: {
          gateway: meta.public_url,
          workspace_id: fixtureWorkspace,
          workspace: 'Default',
          role,
          state: 'pending',
          intent: {
            workspace_id: fixtureWorkspace,
            name: 'dev-mac',
            os: 'macos',
            register_runner: true,
            allowed_roots: ['/projects/中文 workspace'],
          },
        },
      })
    }
    return route.fulfill({ json: [] })
  })
  return calls
}
test('topbar Prompt previews before copying the complete workspace setup instructions', async ({
  page,
}) => {
  await fixture(page)
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', {
      value: {
        writeText: async (s: string) => {
          ;(window as any).copiedPrompt = s
        },
      },
    })
  })
  await page.goto(workspacePage('/'))
  const open = page.getByRole('button', { name: '打开 Agent Prompt', exact: true })
  await open.click()
  const dialog = page.getByRole('dialog', { name: 'Agent Prompt', exact: true })
  await expect(dialog).toContainText('复制以下提示词给 Agent，完成 Wakeit 配置。')
  expect(await page.evaluate(() => (window as any).copiedPrompt)).toBeUndefined()
  const preview = await dialog.getByRole('region', { name: '提示词预览' }).textContent()
  await dialog.getByRole('button', { name: '复制 Agent Prompt', exact: true }).click()
  const text = await page.evaluate(() => (window as any).copiedPrompt as string)
  expect(text).toBe(preview)
  expect(text).toContain(meta.public_url)
  expect(text).toContain('wakeit setup --gateway')
  expect(text).toContain('https://raw.githubusercontent.com/mob999/wakeit/main/install.sh | sh')
  expect(text).toContain('--version 0.2.0 --skill-dir')
  expect(text).not.toContain('download wakeit-release.json, SHA256SUMS')
  expect(text).toContain('local customizations')
  expect(text).not.toMatch(/[\u3400-\u9fff]/)
  expect(text).not.toContain('access_token')
  const version = await page.locator('.brand-version').boundingBox()
  await dialog.getByRole('button', { name: '关闭', exact: true }).click()
  await expect(open).toBeFocused()
  const icon = await open.boundingBox()
  expect(icon!.x).toBeGreaterThan(version!.x)
})
test('clipboard rejection has manual copy without changing Prompt content', async ({ page }) => {
  await fixture(page)
  await page.addInitScript(() =>
    Object.defineProperty(navigator, 'clipboard', {
      value: {
        writeText: async () => {
          throw Error('denied')
        },
      },
    }),
  )
  await page.goto(workspacePage('/'))
  await page.setViewportSize({ width: 390, height: 844 })
  await page.getByRole('button', { name: '打开 Agent Prompt', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: 'Agent Prompt', exact: true })
  await dialog.getByRole('button', { name: '复制 Agent Prompt', exact: true }).click()
  await expect(page.getByRole('alert')).toContainText('请选中文本后复制')
  await expect(dialog).toBeVisible()
  await expect(dialog.getByRole('region', { name: '提示词预览' })).toContainText(meta.public_url)
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy()
})
test('device consent requires explicit action and preserves verification after login', async ({
  page,
}) => {
  const calls = await fixture(page, 'operator', false)
  await page.goto(workspacePage('/authorize/device?user_code=ABCD2345'))
  await page.getByLabel('用户名').fill('tester')
  await page.getByLabel('密码', { exact: true }).fill('fixture-password')
  await page.getByRole('button', { name: '登录工作区' }).click()
  await expect(page.getByText('ABCD2345', { exact: true })).toBeVisible()
  expect(calls).toEqual([])
  await expect(page.getByText('/projects/中文 workspace', { exact: true })).toBeVisible()
  await page.getByRole('button', { name: '授权', exact: true }).click()
  expect(calls).toEqual(['/device-authorizations/ABCD2345/approve'])
  await expect(page.getByRole('status').filter({ hasText: '已授权，可以返回终端' })).toHaveText(
    '已授权，可以返回终端',
  )
})
test('Viewer cannot consent to Runner registration and can reject', async ({ page }) => {
  const calls = await fixture(page, 'viewer')
  await page.goto(workspacePage('/authorize/device?user_code=ABCD2345'))
  await expect(page.getByRole('button', { name: '授权', exact: true })).toBeDisabled()
  await page.getByRole('button', { name: '取消', exact: true }).click()
  expect(calls).toEqual(['/device-authorizations/ABCD2345/deny'])
})
test('authorization can switch language without approving the request', async ({ page }) => {
  const calls = await fixture(page, 'viewer')
  await page.goto(workspacePage('/authorize/device?user_code=ABCD2345'))
  await page.getByRole('button', { name: '切换到英文' }).click()
  await expect(page.getByRole('dialog', { name: 'Connect Wakeit CLI' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Authorize', exact: true })).toBeDisabled()
  await expect(page.getByText('ABCD2345', { exact: true })).toBeVisible()
  await expect(page.getByText('/projects/中文 workspace', { exact: true })).toBeVisible()
  expect(calls).toEqual([])
})
test('consent fits light/dark narrow and desktop layouts', async ({ page }) => {
  await fixture(page)
  for (const width of [390, 1440])
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize({ width, height: 900 })
      await page.addInitScript((t) => localStorage.setItem('wakeit-theme', t), theme)
      await page.goto(workspacePage('/authorize/device?user_code=ABCD2345'))
      await expect(page.getByText('ABCD2345', { exact: true })).toBeVisible()
      const box = await page.getByRole('dialog').boundingBox()
      expect(box!.x).toBeGreaterThanOrEqual(0)
      expect(box!.x + box!.width).toBeLessThanOrEqual(width)
      await page.screenshot({
        path: `../.local/screenshots/device-authorization-${width}-${theme}.png`,
      })
    }
})
