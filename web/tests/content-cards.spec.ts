import { test, expect, type Locator } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
import { loginSession } from './session'
const stack = JSON.parse(
  fs.readFileSync(
    process.env.WAKEIT_TEST_STACK ?? path.resolve('../.local/integration-stack.json'),
    'utf8',
  ),
)
async function cardStyle(card: Locator) {
  const style = await card.evaluate((e) => {
    const s = getComputedStyle(e)
    return { border: s.borderTopWidth, radius: s.borderTopLeftRadius, shadow: s.boxShadow }
  })
  expect(style).toEqual({ border: '1px', radius: '6px', shadow: 'none' })
}
test('all workbenches share bordered containers, surface headers and responsive full-width content', async ({
  page,
}) => {
  await loginSession(page, 'admin', stack.password)
  const task = (await (await page.request.get('/api/v1/tasks?page=1&page_size=1')).json()).items[0]
  const run = (
    await (await page.request.get('/api/v1/runs?state=succeeded&page=1&page_size=1')).json()
  ).items[0]
  const runner = (await (await page.request.get('/api/v1/runners')).json()).find(
    (r: any) => r.name === 'test-device',
  )
  const views = [
    ['task', `/tasks?task=${task.id}`],
    ['run', `/runs?run=${run.id}`],
    ['runner', `/runners?runner=${runner.id}&section=overview`],
    ['agents', `/runners?runner=${runner.id}&section=agents`],
    ['workspace', '/settings?section=workspace'],
    ['members', '/settings?section=members'],
    ['tokens', '/settings?section=tokens'],
  ]
  for (const [name, url] of views) {
    for (const [width, theme] of [
      [1440, 'light'],
      [1440, 'dark'],
      [390, 'light'],
      [2200, 'light'],
    ] as const) {
      await page.setViewportSize({ width, height: 1000 })
      await page.evaluate((theme) => localStorage.setItem('wakeit-theme', theme), theme)
      await page.goto(url)
      const card = page.locator('.detail-pane .content-card:visible').first()
      await expect(card).toBeVisible()
      await cardStyle(card)
      const head = card.locator('.card-header,.table-head').first()
      await expect(head).toBeVisible()
      expect((await head.boundingBox())!.height).toBe(44)
      expect(await head.evaluate((e) => getComputedStyle(e).backgroundColor)).toBe(
        theme === 'dark' ? 'rgb(23, 23, 23)' : 'rgb(245, 245, 245)',
      )
      const main = page
        .locator('.detail-pane .run-page:visible,.detail-pane .detail-scroll:visible')
        .first()
      const box = (await card.boundingBox())!
      const region = (await main.boundingBox())!
      expect(box.width).toBeGreaterThan(region.width - 45)
      expect(box.x + box.width).toBeLessThanOrEqual(width)
      expect(
        await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
      ).toBeTruthy()
      if (name === 'task') {
        await expect(
          page.locator('.detail-scroll > .content-card').nth(1).locator('.card-header'),
        ).toHaveText('指令')
        await expect(page.locator('.task-prompt')).toHaveText(task.config.prompt)
        await expect(page.locator('.run-table-head')).toBeVisible()
        await expect(page.locator('.run-table > .card-header')).toHaveCount(0)
      }
      if (width !== 2200)
        await page.screenshot({
          path: `test-results/cards-${name}-${width}-${theme}.png`,
          fullPage: true,
        })
    }
  }
})
test('login and dialogs share the same frame and preserve compact accessible controls', async ({
  page,
}) => {
  await page.goto('/')
  await cardStyle(page.locator('.login-card'))
  await expect(page.locator('.login-card > .card-header')).toBeVisible()
  await page.screenshot({ path: 'test-results/cards-login-light.png' })
  await loginSession(page, 'admin', stack.password)
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  const dialog = page.getByRole('dialog')
  expect(await dialog.evaluate((e) => getComputedStyle(e).borderRadius)).toBe('6px')
  expect(
    await dialog.locator('.modal-heading').evaluate((e) => getComputedStyle(e).backgroundColor),
  ).toBe('rgb(245, 245, 245)')
  await page.screenshot({ path: 'test-results/cards-editor-light.png' })
  await dialog.getByRole('button', { name: '取消', exact: true }).click()
  await page.getByRole('button', { name: '搜索与筛选' }).click()
  const search = page.getByRole('dialog', { name: '搜索', exact: true })
  const group = search.locator('.search-group').first()
  await expect(group).toBeVisible()
  await cardStyle(group)
  await expect(group.locator('.table-head')).toBeVisible()
  await page.screenshot({ path: 'test-results/cards-search-light.png' })
})
