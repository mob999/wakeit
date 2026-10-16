import { expect, test, type Page } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'

const english = fs.readFileSync(path.resolve('../CHANGELOG.md'), 'utf8')
const chinese = fs.readFileSync(path.resolve('../CHANGELOG.zh-CN.md'), 'utf8')
async function fixture(page: Page, { role = 'admin', failures = 0, long = false } = {}) {
  const calls: string[] = []
  await page.route('**/api/v1/**', async (route) => {
    const url = new URL(route.request().url())
    const name = url.pathname.replace('/api/v1', '')
    const send = (json: unknown, status = 200) => route.fulfill({ json, status })
    if (name === '/session') return send({ id: 'fixture-user', username: 'fixture', role })
    if (name === '/meta')
      return send({ version: '0.1.0', public_url: 'https://wakeit.example.test', install: {} })
    if (name === '/settings')
      return send({ timezone: 'UTC', log_retention_days: 7, run_retention_days: 90 })
    if (name === '/moments/days')
      return send({
        timezone: 'UTC',
        today: '2026-10-16',
        total_runs: 0,
        days: [],
        next_before: null,
      })
    if (name === '/changelog') {
      const language = url.searchParams.get('lang')!
      calls.push(language)
      if (failures-- > 0) return send({ error: 'fixture failure' }, 500)
      const content = language === 'zh' ? chinese : english
      return send({
        version: '0.1.0',
        content: content + (long ? '\n## 0.0.9\n\n' + '- Historical change.\n'.repeat(100) : ''),
      })
    }
    return send([])
  })
  return calls
}

for (const role of ['admin', 'operator', 'viewer']) {
  test(`${role} opens notes on demand with keyboard and restores focus`, async ({ page }) => {
    const calls = await fixture(page, { role })
    await page.goto('/')
    const trigger = page.getByRole('button', { name: '更新日志 0.1.0', exact: true })
    await expect(trigger).toBeVisible()
    expect(calls).toEqual([])
    await trigger.focus()
    await trigger.press('Enter')
    const dialog = page.getByRole('dialog', { name: '更新日志', exact: true })
    await expect(dialog.getByRole('heading', { name: '0.1.0', exact: true })).toBeVisible()
    await expect(dialog).toContainText('支持 Cron')
    await page.keyboard.press('Escape')
    await expect(dialog).not.toBeVisible()
    await expect(trigger).toBeFocused()
    await trigger.click()
    await expect(dialog).toBeVisible()
    expect(calls).toEqual(['zh'])
  })
}

test('language switches in the open dialog and keeps separate cached documents', async ({
  page,
}) => {
  const calls = await fixture(page)
  await page.goto('/')
  await page.getByRole('button', { name: '更新日志 0.1.0', exact: true }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog).toContainText('支持 Cron')
  await dialog.getByRole('button', { name: '切换到英文' }).click()
  await expect(dialog).toHaveAccessibleName('Changelog')
  await expect(dialog).toContainText('Schedule Agent tasks')
  await expect(dialog).not.toContainText('支持 Cron')
  await dialog.getByRole('button', { name: 'Switch to Chinese' }).click()
  await expect(dialog).toContainText('支持 Cron')
  expect(calls).toEqual(['zh', 'en'])
})

test('failed loading can be retried without closing the dialog', async ({ page }) => {
  const calls = await fixture(page, { failures: 1 })
  await page.goto('/')
  await page.getByRole('button', { name: '更新日志 0.1.0', exact: true }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('alert')).toContainText('无法加载更新日志')
  await dialog.getByRole('button', { name: '重试', exact: true }).click()
  await expect(dialog).toContainText('支持 Cron')
  expect(calls).toEqual(['zh', 'zh'])
})

for (const [name, width, height, dark] of [
  ['desktop-light', 1440, 900, false],
  ['desktop-dark', 1440, 900, true],
  ['mobile-light', 390, 844, false],
  ['mobile-dark', 390, 844, true],
  ['wide-light', 2400, 1200, false],
] as const) {
  test(`${name}: notes fit the viewport with scrolling inside the dialog`, async ({ page }) => {
    await page.setViewportSize({ width, height })
    await fixture(page, { long: true })
    await page.goto('/')
    if (dark) await page.getByRole('button', { name: '切换主题', exact: true }).click()
    await page.getByRole('button', { name: '更新日志 0.1.0', exact: true }).click()
    const dialog = page.getByRole('dialog')
    await expect(dialog.getByRole('heading', { name: '0.1.0', exact: true })).toBeVisible()
    const box = (await dialog.boundingBox())!
    expect(box.x).toBeGreaterThanOrEqual(15)
    expect(box.x + box.width).toBeLessThanOrEqual(width - 15)
    expect(box.y).toBeGreaterThanOrEqual(0)
    expect(box.y + box.height).toBeLessThanOrEqual(height)
    const body = dialog.locator('.changelog-body')
    expect(await body.evaluate((el) => el.scrollHeight > el.clientHeight)).toBe(true)
    await body.evaluate((el) => {
      el.scrollTop = el.scrollHeight
    })
    await expect(dialog.getByRole('button', { name: '关闭', exact: true })).toBeVisible()
    await body.evaluate((el) => {
      el.scrollTop = 0
    })
    fs.mkdirSync(path.resolve('../.local/changelog/screenshots'), { recursive: true })
    await page.screenshot({ path: path.resolve(`../.local/changelog/screenshots/${name}.png`) })
  })
}
