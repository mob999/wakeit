import { test, expect, type Page } from '@playwright/test'
import { selectChoice } from './choices'
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
                      : path.endsWith('/directories')
                        ? {
                            path: '/workspace',
                            parent: null,
                            breadcrumbs: [{ name: 'workspace', path: '/workspace' }],
                            directories: [],
                            next_cursor: null,
                          }
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
  await page.getByRole('option', { name: /^Collect sources/ }).click()
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: '引用上游结果', exact: true }).click()
  await page.getByRole('menuitem', { name: /^Collect sources/ }).click()
  await expect(page.getByLabel('指令', { exact: true })).toHaveValue(/nodes\..+\.result/)
  await page.locator('.dag-graph-node').filter({ hasText: 'Collect sources' }).click()
  await expect(page.getByLabel('指令', { exact: true })).toHaveValue('Find primary sources\n中文')
  await page
    .locator('.dag-node-context')
    .getByRole('button', { name: '节点操作', exact: true })
    .click()
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

test('DAG Next only switches steps; failed saves and revision conflicts retain the complete draft', async ({
  page,
}) => {
  await mock(page)
  await page.goto(`/w/${workspace}/tasks/dag`)
  await page.getByLabel('任务名称', { exact: true }).fill('Daily report')
  await page.getByLabel('名称', { exact: true }).fill('Collect')
  await page.getByLabel('指令', { exact: true }).fill('Keep this entire instruction')
  await selectChoice(page, page, 'Runner', 'Preview Runner')
  await selectChoice(page, page, 'Agent', 'Pi')
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.getByRole('dialog').getByRole('button', { name: '选择文件夹', exact: true }).click()
  const writes: any[] = []
  await page.route('**/api/v1/workspaces/*/tasks/*', async (route) => {
    if (route.request().method() !== 'PUT') return route.fallback()
    writes.push(route.request().postDataJSON())
    await route.fulfill({
      status: writes.length === 1 ? 503 : 409,
      json: { error: writes.length === 1 ? '保存失败' : '版本冲突' },
    })
  })
  await page.getByRole('button', { name: '下一步', exact: true }).click()
  await expect(page.getByRole('tab', { name: '时间', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  expect(writes).toHaveLength(0)
  await selectChoice(page, page, '类型', 'manual')
  await page.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(page.getByRole('alert')).toContainText('保存失败')
  await page.getByRole('button', { name: '上一步', exact: true }).click()
  await expect(page.getByLabel('指令', { exact: true })).toHaveValue('Keep this entire instruction')
  await page.getByRole('button', { name: '下一步', exact: true }).click()
  await page.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(page.getByRole('alert')).toContainText('版本冲突')
  expect(writes[0].config.dag.nodes).toEqual(writes[1].config.dag.nodes)
  expect(writes[0].config.id).toBe(writes[1].config.id)
  expect(writes[1].config.schedule).toBeNull()
})

test('DAG output selection switches between automatic leaves and an explicit subset', async ({
  page,
}) => {
  await mock(page)
  await page.goto(`/w/${workspace}/tasks/dag`)
  await page.getByLabel('名称', { exact: true }).fill('Output A')
  await page.getByRole('button', { name: '新增节点', exact: true }).click()
  await page.getByLabel('名称', { exact: true }).fill('Output B')
  await selectChoice(page, page, '结果选择', '指定节点')
  await page.getByRole('combobox', { name: '结果节点', exact: true }).click()
  await expect(page.getByRole('option', { name: /^Output A/ })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await page.getByRole('option', { name: /^Output B/ }).click()
  await expect(page.getByRole('option', { name: /^Output A/ })).toBeDisabled()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('combobox', { name: '结果节点', exact: true })).toContainText(
    'Output A',
  )
  await expect(
    page.locator('.dag-graph-node').filter({ hasText: 'Output A' }).getByLabel('结果节点'),
  ).toHaveCount(1)
  await selectChoice(page, page, '结果选择', '末端节点（自动）')
  await expect(page.locator('.dag-graph-node').getByLabel('结果节点')).toHaveCount(2)
})

for (const count of [20, 100])
  test(`${count}-node DAG keeps an accessible list and bounded layout`, async ({ page }) => {
    await mock(page)
    const nodes = Array.from({ length: count }, (_, index) => ({
      id: `node-${index}`,
      name: `Research step ${index} 中文 long name`,
      pre: index > 1 ? [`node-${Math.floor((index - 1) / 2)}`] : [],
      runner: runner.id,
      agent: 'pi',
      cwd: '/workspace',
      prompt: 'Research this step',
      parameters: {},
      timeout: '1h',
    }))
    const config = {
      version: 1,
      id: 'layout-fixture',
      name: 'Synthetic research graph',
      dag: { nodes },
      schedule: null,
      enabled: true,
      execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
    }
    await page.route('**/tasks/layout-fixture', (route) =>
      route.fulfill({ json: { id: config.id, config, revision: 1, archived_at: null } }),
    )
    await page.goto(`/w/${workspace}/tasks/dag?task=layout-fixture`)
    await expect(page.locator('.dag-graph-node')).toHaveCount(count)
    await page.getByRole('button', { name: '表格', exact: true }).click()
    const list = page.getByRole('table', { name: '节点', exact: true })
    await expect(list.locator('.dag-list-node')).toHaveCount(count)
    const first = list.locator('.dag-list-node').first()
    await first.focus()
    await page.keyboard.press('ArrowDown')
    await page.keyboard.press('Enter')
    await expect(list.locator('.dag-list-node').nth(1)).toBeFocused()
    await expect(page.getByLabel('名称', { exact: true })).toHaveValue(nodes[1].name)
    await page.getByRole('button', { name: '搜索节点', exact: true }).click()
    await page.getByRole('textbox', { name: '搜索节点', exact: true }).fill(`step ${count - 1} `)
    await expect(list.locator('.dag-list-node')).toHaveCount(1)
    await page.keyboard.press('Escape')
    for (const width of [390, 1440, 2200]) {
      await page.setViewportSize({ width, height: 900 })
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
        true,
      )
    }
  })
