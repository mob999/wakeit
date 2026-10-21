import { test, expect, type Page } from '@playwright/test'
import { workspacePage, mockWorkspace, apiPath } from './workspace'
const id = '00000000-0000-4000-8000-000000000001'
async function fixture(page: Page, role = 'admin') {
  const reads: any[] = []
  const config = {
    version: 1,
    id: 'files',
    name: 'File fixture',
    runner: 'Preview device',
    agent: 'pi',
    cwd: '/workspace',
    prompt: 'Synthetic instruction',
    parameters: {},
    enabled: true,
    schedule: null,
    execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
  }
  const run = {
    id,
    task_id: 'files',
    config,
    state: 'succeeded',
    source: 'manual',
    created_at: '2026-10-20T01:00:00Z',
    finished_at: '2026-10-20T01:00:01Z',
    result:
      '[Report](/workspace/report.md)\n\n<a href="file:///workspace/html.md">HTML report</a>\n\n[Windows](C:/data/report.md)\n\n![Local image](/workspace/picture.png)',
    execution_snapshot: { trace_supported: false, cwd: '/workspace' },
  }
  await page.route('**/api/v1/**', async (route) => {
    if (await mockWorkspace(route, role)) return
    const path = apiPath(new URL(route.request().url()).pathname)
    if (path === '/session')
      return route.fulfill({ json: { id: 'preview', username: 'preview', role } })
    if (path === '/meta') return route.fulfill({ json: { version: '0.3.0' } })
    if (path === '/settings') return route.fulfill({ json: { timezone: 'UTC' } })
    if (path === '/runners')
      return route.fulfill({
        json: [{ id: 'device', name: 'Preview device', online: true, agents: [] }],
      })
    if (path === '/runs')
      return route.fulfill({ json: { items: [run], total: 1, page: 1, page_size: 20 } })
    if (path === `/runs/${id}`) return route.fulfill({ json: run })
    if (path.endsWith('/files/preview')) {
      reads.push(route.request().postDataJSON())
      return route.fulfill({
        json: {
          path: '/workspace/report.md',
          name: 'report.md',
          runner_id: 'device',
          format: 'markdown',
          mime: 'text/plain',
          content: '# Preview\n\n[Nested](./nested.md)',
          size: 33,
          modified_at: null,
          truncated: false,
        },
      })
    }
    if (path.endsWith('/stream'))
      return route.fulfill({ contentType: 'text/event-stream', body: ':fixture\n\n' })
    return route.fulfill({ json: [] })
  })
  await page.goto(workspacePage(`/runs?run=${id}`))
  return reads
}
test('local result links preview only on click and preview links do not expand file authorization', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  const reads = await fixture(page)
  await expect(page.getByRole('button', { name: 'Report', exact: true })).toBeVisible()
  expect(reads).toEqual([])
  expect(await page.locator('img[src="/workspace/picture.png"]').count()).toBe(0)
  await page.getByRole('button', { name: 'Report', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: 'report.md', exact: true })
  await expect(dialog.getByRole('heading', { name: 'Preview', exact: true })).toBeVisible()
  expect(reads).toEqual([{ href: '/workspace/report.md' }])
  await dialog.getByRole('button', { name: 'Nested', exact: true }).click()
  expect(reads).toHaveLength(1)
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe('./nested.md')
  for (const width of [390, 1440])
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize({ width, height: 900 })
      await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
      expect(
        await dialog.evaluate(
          (e) =>
            e.scrollWidth <= e.clientWidth &&
            e.getBoundingClientRect().left >= 7 &&
            e.getBoundingClientRect().right <= innerWidth,
        ),
      ).toBe(true)
      await page.screenshot({ path: `test-results/file-${width}-${theme}.png` })
    }
})
test('file and Windows targets survive sanitization without becoming Gateway paths', async ({
  page,
}) => {
  const reads = await fixture(page)
  await page.getByRole('button', { name: 'HTML report', exact: true }).click()
  await expect(page.getByRole('dialog')).toBeVisible()
  expect(reads[0]).toEqual({ href: 'file:///workspace/html.md' })
  await page.getByRole('dialog').getByRole('button', { name: '关闭', exact: true }).click()
  await page.getByRole('button', { name: 'Windows', exact: true }).click()
  await expect.poll(() => reads.length).toBe(2)
  expect(reads[1]).toEqual({ href: 'C:/data/report.md' })
})
test('Viewer can copy result paths without file IO', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  const reads = await fixture(page, 'viewer')
  await page.getByRole('button', { name: 'Report', exact: true }).click()
  await expect
    .poll(() => page.evaluate(() => navigator.clipboard.readText()))
    .toBe('/workspace/report.md')
  expect(reads).toEqual([])
  await expect(page.getByRole('dialog')).toHaveCount(0)
})
