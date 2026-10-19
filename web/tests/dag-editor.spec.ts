import { test, expect, type Page } from '@playwright/test'
const workspace = '11111111-1111-4111-8111-111111111111'
const agent = {
  id: 'pi',
  name: 'Pi',
  available: true,
  mode: 'sync',
  parameters: {},
  models_supported: false,
  cancellable: true,
  digest: 'fixture',
}
const runner = {
  id: '22222222-2222-4222-8222-222222222222',
  name: 'Preview Runner',
  online: true,
  revoked: false,
  agents: [agent],
  directory_roots: ['/workspace'],
  info: {},
}
async function mock(page: Page) {
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    const value =
      path === '/api/v1/session'
        ? { id: 'test-user', username: 'preview', role: 'admin' }
        : path === '/api/v1/meta'
          ? { version: '0.2.0' }
          : path === `/api/v1/workspaces/${workspace}`
            ? { id: workspace, name: 'Preview', role: 'admin', is_demo: false }
            : path.endsWith('/workspaces')
              ? { items: [{ id: workspace, name: 'Preview', role: 'admin' }], demo: null }
              : path.endsWith('/settings')
                ? { timezone: 'UTC' }
                : path.endsWith('/runners')
                  ? [runner]
                  : path.endsWith('/tasks/validate')
                    ? { errors: [] }
                    : path.endsWith('/schedule/preview')
                      ? ['2026-10-20T09:00:00Z']
                      : []
    await route.fulfill({ json: value })
  })
}
test('DAG prototype keeps node drafts, renders worker graph and protects referenced nodes', async ({
  page,
}) => {
  await mock(page)
  await page.goto(`/w/${workspace}/tasks/dag`)
  await page.getByLabel('任务名称', { exact: true }).fill('Daily research')
  await page.getByLabel('名称', { exact: true }).fill('Collect sources')
  await page.getByLabel('指令', { exact: true }).fill('Find primary sources\n中文')
  await expect(page.locator('.dag-graph-node')).toHaveCount(1)
  await page.getByRole('button', { name: '新增节点', exact: true }).click()
  await page.getByLabel('名称', { exact: true }).fill('Write report')
  await page.getByRole('combobox', { name: '前置节点', exact: true }).click()
  await page.getByRole('option', { name: 'Collect sources', exact: true }).click()
  await page.keyboard.press('Escape')
  await page.getByRole('combobox', { name: '引用上游结果', exact: true }).click()
  await page.getByRole('option', { name: 'Collect sources', exact: true }).click()
  await expect(page.getByLabel('指令', { exact: true })).toHaveValue(/nodes\..+\.result/)
  await page.locator('.dag-graph-node').filter({ hasText: 'Collect sources' }).click()
  await expect(page.getByLabel('指令', { exact: true })).toHaveValue('Find primary sources\n中文')
  await page.getByRole('button', { name: '节点操作', exact: true }).click()
  await page.getByRole('menuitem', { name: '删除节点', exact: true }).click()
  await expect(
    page.getByRole('dialog').getByRole('button', { name: '删除节点', exact: true }),
  ).toBeDisabled()
  await page.getByRole('dialog').getByRole('button', { name: '取消', exact: true }).click()
  for (const [width, theme] of [
    [1440, 'light'],
    [390, 'light'],
    [1440, 'dark'],
    [2200, 'dark'],
  ] as const) {
    await page.setViewportSize({ width, height: 900 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect
      .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
      .toBe(true)
    await page.screenshot({ path: `test-results/dag-editor-${width}-${theme}.png`, fullPage: true })
  }
})
