import { test, expect, type Page } from '@playwright/test'
import { workspacePage, apiPath, mockWorkspace } from './workspace'
const at = '2026-10-20T09:00:00Z'
async function fixture(page: Page) {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  const writes: string[] = []
  await page.route('**/api/v1/**', async (route) => {
    if (await mockWorkspace(route)) return
    const url = new URL(route.request().url())
    const path = apiPath(url.pathname),
      method = route.request().method()
    if (method === 'PUT') writes.push(path)
    const send = (json: unknown, status = 200) => route.fulfill({ json, status })
    if (path === '/session')
      return send({ id: 'preview-user', username: 'preview', role: 'admin', instance_admin: true })
    if (path === '/meta') return send({ version: '0.2.0' })
    if (path === '/settings')
      return send({ timezone: 'UTC', log_retention_days: 7, run_retention_days: 90 })
    if (path === '/timezones') return send(['UTC'])
    if (path === '/schedule/preview') return send([at])
    if (path === '/tasks/validate') return send({ errors: [] })
    if (path === '/members' || path === '/instance/users')
      return send({
        items: [
          { id: 'preview-user', username: 'Preview admin', role: 'admin', active: true },
          { id: 'writer', username: 'Preview writer', role: 'operator', active: true },
        ],
        total: 2,
        page: 1,
        page_size: 20,
        active_admins: 1,
      })
    if (path === '/tokens')
      return send({
        items: [
          { id: 'key-a', name: 'Development key', expires_at: null, created_at: at },
          { id: 'key-b', name: 'Temporary key', expires_at: at, created_at: at },
        ],
        total: 2,
        page: 1,
        page_size: 20,
      })
    if (path.startsWith('/tasks'))
      return send({
        items:
          path === '/tasks' && url.searchParams.get('archived') === 'true'
            ? Array.from({ length: 2 }, (_, i) => ({
                id: `task-${i}`,
                config: { name: `Archived task ${i}`, runner: 'Preview device', agent: 'pi' },
                archived_at: at,
                revision: 1,
              }))
            : [],
        total: 2,
        page: 1,
        page_size: 20,
      })
    if (path === '/agents')
      return send([{ id: 'pi', name: 'Pi', runner: 'Preview device', logo: '/agent-logos/pi.svg' }])
    if (path === '/runners')
      return send([
        {
          id: 'preview-device',
          name: 'Preview device',
          online: true,
          revoked: false,
          agents: [{ id: 'pi', name: 'Pi', available: true, parameters: {}, mode: 'sync' }],
          directory_roots: ['/workspace'],
        },
      ])
    if (path === '/moments/days')
      return send({
        timezone: 'UTC',
        today: '2026-10-20',
        total_runs: 0,
        days: [],
        next_before: null,
      })
    if (path.endsWith('/leave')) return send({ error: 'cannot remove the last administrator' }, 409)
    return send([])
  })
  return writes
}
test('Settings shares typography, cell tracks and action alignment across every table', async ({
  page,
}) => {
  await fixture(page)
  for (const [section, selector] of [
    ['members', '.settings-member-row'],
    ['tokens', '.settings-token-row'],
    ['archived', '.archive-row'],
    ['instance', '.settings-member-row'],
  ] as const) {
    await page.goto(
      section === 'instance' ? '/instance' : workspacePage(`/settings?section=${section}`),
    )
    const table = page.locator('.settings-table'),
      header = table.locator('.table-head'),
      row = table.locator(selector).nth(1)
    await expect(row).toBeVisible()
    const headers = header.locator(':scope > span'),
      cells = row.locator(':scope > *')
    const count = await headers.count()
    for (let column = 0; column < count - 1; column++) {
      const h = (await headers.nth(column).boundingBox())!,
        cell = (await cells.nth(column).boundingBox())!
      expect(cell.x).toBeCloseTo(h.x, 0)
      expect(await headers.nth(column).evaluate((el) => getComputedStyle(el).fontSize)).toBe('11px')
      expect(await cells.nth(column).evaluate((el) => getComputedStyle(el).fontSize)).toBe('12px')
    }
    const h = (await headers.last().boundingBox())!,
      action = (await cells.last().boundingBox())!
    expect(h.x + h.width / 2).toBeCloseTo(action.x + action.width / 2, 0)
    if (section === 'members')
      expect(
        await row.locator('.settings-role-button').evaluate((el) => getComputedStyle(el).fontSize),
      ).toBe('12px')
    await page.screenshot({ path: `test-results/settings-${section}-aligned.png`, fullPage: true })
    await page.setViewportSize({ width: 390, height: 900 })
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await expect
      .poll(() =>
        row.evaluate((el) => {
          const headings = el.closest('.settings-table')!.querySelector('.table-head')!.children
          return Math.max(
            ...Array.from(el.children)
              .slice(0, -1)
              .map((cell, i) =>
                Math.abs(cell.getBoundingClientRect().x - headings[i].getBoundingClientRect().x),
              ),
          )
        }),
      )
      .toBeLessThan(1)
    await page.screenshot({ path: `test-results/settings-${section}-390.png`, fullPage: true })
    await page.setViewportSize({ width: 1440, height: 900 })
  }
  await page.goto(workspacePage('/settings?section=workspace'))
  const card = page.locator('.workspace-settings')
  await expect(card.locator('.form-card-head')).toHaveText('设置')
  await expect(card.locator('.workspace-management-actions')).toHaveCount(0)
  await expect(
    card.locator('.form-card-head').getByRole('button', { name: '创建工作区', exact: true }),
  ).toBeVisible()
  await expect(
    card.locator('.workspace-name-row').getByRole('button', { name: '退出工作区', exact: true }),
  ).toBeVisible()
  const rows = card.locator('.setting-row')
  for (let i = 0; i < (await rows.count()); i++) {
    const label = rows.nth(i).locator(':scope > *').first()
    expect(await label.evaluate((el) => getComputedStyle(el).fontSize)).toBe('12px')
    const value = rows.nth(i).locator(':scope > *').last()
    const first = (await rows.first().locator(':scope > *').last().boundingBox())!
    expect((await value.boundingBox())!.x).toBeCloseTo(first.x, 0)
  }
  for (const [width, dark] of [
    [1440, false],
    [390, false],
    [1440, true],
  ] as const) {
    await page.setViewportSize({ width, height: 900 })
    await page.evaluate(
      (dark) => (document.documentElement.dataset.theme = dark ? 'dark' : 'light'),
      dark,
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/settings-workspace-${width}-${dark ? 'dark' : 'light'}.png`,
      fullPage: true,
    })
  }
})
test('errors appear only in the top-right Toast while the failed dialog stays open', async ({
  page,
}) => {
  await fixture(page)
  await page.goto(workspacePage('/settings?section=workspace'))
  await page.getByRole('button', { name: '退出工作区', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '退出工作区', exact: true })
  await dialog.getByRole('button', { name: '退出', exact: true }).click()
  const toast = page.getByRole('alert')
  await expect(toast).toContainText('必须至少保留一名有效管理员')
  await expect(dialog.locator('[role=alert], .error')).toHaveCount(0)
  await expect(dialog).toBeVisible()
  const box = (await toast.boundingBox())!
  expect(box.y).toBeLessThan(100)
  expect(box.x + box.width).toBeGreaterThan(1400)
  await page.getByRole('button', { name: '关闭提示', exact: true }).click()
  await expect(toast).toHaveCount(0)
  await expect(dialog).toBeVisible()
})
test('ordinary and DAG drafts switch freely; only Save validates and keeps the draft', async ({
  page,
}) => {
  const writes = await fixture(page)
  await page.goto(workspacePage('/tasks'))
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  await page.getByRole('menuitem', { name: '任务', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '新建任务', exact: true })
  await dialog.getByRole('tab', { name: '时间', exact: true }).click()
  await expect(dialog.getByRole('tab', { name: '时间', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await expect(page.getByRole('alert')).toHaveCount(0)
  await dialog.getByLabel('Cron 表达式', { exact: true }).fill('30 10 * * *')
  await dialog.getByRole('tab', { name: '任务', exact: true }).click()
  await dialog.getByRole('button', { name: '下一步', exact: true }).click()
  await expect(dialog.getByLabel('Cron 表达式', { exact: true })).toHaveValue('30 10 * * *')
  await dialog.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(page.getByRole('alert')).toHaveText('请填写任务名称')
  await expect(dialog.getByLabel('任务名称', { exact: true })).toBeFocused()
  await dialog.getByRole('button', { name: '取消', exact: true }).click()
  await page.getByRole('button', { name: '关闭提示', exact: true }).click()
  await page.goto(workspacePage('/tasks/dag'))
  await page.getByRole('tab', { name: '时间', exact: true }).click()
  await expect(page.getByRole('tab', { name: '时间', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await expect(page.getByRole('alert')).toHaveCount(0)
  await page.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(page.getByRole('alert')).toHaveText('请填写任务名称')
  await expect(page.getByLabel('任务名称', { exact: true })).toBeFocused()
  expect(writes).toEqual([])
})
