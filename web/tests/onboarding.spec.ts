import { workspacePage, apiPath, mockWorkspace, fixtureWorkspace } from './workspace'
import { test, expect, type Page } from '@playwright/test'
const meta = {
  version: '0.1.1',
  public_url: 'https://wakeit.example.test',
  install: {
    version: '0.1.1',
    release: 'https://github.com/mob999/wakeit/releases/tag/v0.1.1',
    manifest: 'https://github.com/mob999/wakeit/releases/download/v0.1.1/wakeit-release.json',
    checksums: 'https://github.com/mob999/wakeit/releases/download/v0.1.1/SHA256SUMS',
    base_url: 'https://github.com/mob999/wakeit/releases/download/v0.1.1',
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
          workspace_id: fixtureWorkspace, workspace: 'Default', role,
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
test('topbar Prompt copies configured Gateway and verified public installation instructions without secrets', async ({
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
  await page.getByRole('button', { name: '复制 Agent Prompt' }).click()
  const text = await page.evaluate(() => (window as any).copiedPrompt as string)
  expect(text).toContain(meta.public_url)
  expect(text).toContain('wakeit setup --gateway')
  expect(text).toContain('SHA-256')
  expect(text).toContain('wakeit-skill.tar.gz')
  expect(text).toContain('SKILL.md, references/ and assets/')
  expect(text).not.toMatch(/[\u3400-\u9fff]/)
  expect(text).not.toContain('access_token')
  const version = await page.locator('.brand-version').boundingBox()
  const icon = await page.getByRole('button', { name: '复制 Agent Prompt' }).boundingBox()
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
  await page.getByRole('button', { name: '复制 Agent Prompt' }).click()
  await expect(page.getByRole('dialog', { name: 'Agent Prompt' })).toBeVisible()
  await expect(page.getByRole('textbox', { name: 'Agent Prompt' })).toContainText(meta.public_url)
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
  await expect(page.getByRole('status')).toHaveText('已授权，可以返回终端')
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
