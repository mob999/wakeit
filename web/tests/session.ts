import { expect, type Page, type Cookie } from '@playwright/test'
const sessions = new Map<string, Cookie[]>()
// Each test still gets a fresh browser context; reuse authenticated cookies to avoid
// exhausting the real Gateway's login rate limit during a full browser suite.
export async function loginSession(page: Page, username: string, password: string) {
  const cookies = sessions.get(username)
  if (cookies) {
    await page.context().addCookies(cookies)
    await page.goto('/')
  } else {
    await page.goto('/')
    await page.getByLabel('用户名', { exact: true }).fill(username)
    await page.getByLabel('密码', { exact: true }).fill(password)
    await page.getByRole('button', { name: '登录工作区' }).click()
    await expect(page.getByRole('heading', { name: '动态', exact: true })).toBeVisible()
    sessions.set(username, await page.context().cookies())
  }
  await page.goto('/tasks')
  await expect(page.locator('.list-pane h1')).toContainText('任务')
}
