import { test, expect } from '@playwright/test'
import { selectChoice } from './choices'
import { base, workspace, runnerId, definition, fixture, editNode } from './dag-fixture'

test('empty draft adds Agents explicitly, keeps instruction drafts and preserves broken references after confirmation', async ({
  page,
}) => {
  await fixture(page)
  await page.goto(base)
  await expect(page.locator('.react-flow__node')).toHaveCount(0)
  await page.getByLabel('任务名称', { exact: true }).fill('Daily research')
  await page.getByRole('button', { name: 'Preview device 上的 Pi', exact: true }).click()
  await expect(page.getByLabel('名称', { exact: true })).toBeFocused()
  await page.getByLabel('名称', { exact: true }).fill('Collect sources')
  await page.getByLabel('指令', { exact: true }).fill('Find primary sources\n中文')
  await page.getByRole('button', { name: '新增节点', exact: true }).click()
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Preview device 上的 Pi', exact: true })
    .click()
  await page.getByLabel('名称', { exact: true }).fill('Write report')
  await page.getByRole('combobox', { name: '前置节点', exact: true }).click()
  await page.getByRole('option', { name: /^Collect sources/ }).click()
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: '引用上游结果', exact: true }).click()
  await page.getByRole('menuitem', { name: /^Collect sources/ }).click()
  await expect(page.getByLabel('指令', { exact: true })).toHaveValue(/nodes\..+\.result/)
  await page
    .locator('.dag-floating-header')
    .getByRole('combobox', { name: '选择节点', exact: true })
    .click()
  await page.getByRole('option', { name: /^Collect sources/ }).click()
  await expect(page.getByLabel('指令', { exact: true })).toHaveValue('Find primary sources\n中文')
  await page
    .locator('.dag-floating-header')
    .getByRole('button', { name: '节点操作', exact: true })
    .click()
  await page.getByRole('menuitem', { name: '删除节点', exact: true }).click()
  await expect(
    page.getByRole('dialog').getByRole('button', { name: '删除节点', exact: true }),
  ).toBeEnabled()
  await page.getByRole('dialog').getByRole('button', { name: '取消', exact: true }).click()
  for (const [width, theme] of [
    [1440, 'light'],
    [390, 'light'],
    [1440, 'dark'],
    [2200, 'dark'],
  ] as const) {
    await page.setViewportSize({ width, height: 900 })
    await page.evaluate(
      () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))),
    )
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({ path: `test-results/dag-canvas-panel-${width}-${theme}.png` })
  }
})

test('Next and tabs do not validate; network and revision failures keep fields and shared positions', async ({
  page,
}) => {
  const f = await fixture(page)
  const writes: any[] = []
  await page.route('**/api/v1/workspaces/*/tasks/*', async (route) => {
    if (route.request().method() !== 'PUT') return route.fallback()
    writes.push(route.request().postDataJSON())
    await route.fulfill({
      status: writes.length === 1 ? 503 : 409,
      json: { error: writes.length === 1 ? '保存失败' : '版本冲突' },
    })
  })
  await page.goto(base)
  await page.getByRole('button', { name: '下一步', exact: true }).click()
  expect(f.requests.filter((r) => r.path === '/tasks/validate')).toHaveLength(0)
  await expect(page.getByRole('alert')).toHaveCount(0)
  await page.getByRole('tab', { name: '任务', exact: true }).click()
  await page.getByLabel('任务名称', { exact: true }).fill('Daily report')
  await page.getByRole('button', { name: 'Preview device 上的 Pi', exact: true }).click()
  await page.getByLabel('指令', { exact: true }).fill('Keep this entire instruction')
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.getByRole('dialog').getByRole('button', { name: '选择文件夹', exact: true }).click()
  await page.getByRole('button', { name: '下一步', exact: true }).click()
  expect(writes).toHaveLength(0)
  await selectChoice(page, page, '类型', '手动执行')
  await page.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(page.getByRole('alert')).toContainText('保存失败')
  await page.getByRole('button', { name: '上一步', exact: true }).click()
  await expect(page.getByLabel('指令', { exact: true })).toHaveValue('Keep this entire instruction')
  await page.getByRole('button', { name: '下一步', exact: true }).click()
  await page.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(page.getByRole('alert')).toContainText('版本冲突')
  expect(writes[1].config).toEqual(writes[0].config)
  expect(writes[1].editor_layout).toEqual(writes[0].editor_layout)
  expect(writes[1].config.schedule).toBeNull()
  expect(writes[1].editor_layout.version).toBe(1)
  expect(Object.keys(writes[1].editor_layout.positions)).toHaveLength(1)
})

test('output selection allows an empty draft, marks actual leaves and blocks an invalid save', async ({
  page,
}) => {
  const f = await fixture(page)
  await page.goto(base + '?task=ux-fixture')
  await expect(page.locator('.dag-interactive-canvas')).toHaveAttribute('data-layout-ready', 'true')
  await page.getByRole('button', { name: '结果设置', exact: true }).click()
  await expect(page.locator('.dag-output-rows')).toContainText('Write report')
  await selectChoice(page, page, '结果选择', '指定节点')
  await page.getByRole('combobox', { name: '结果节点', exact: true }).click()
  await page.getByRole('option', { name: /^Write report/ }).click()
  await expect(page.getByRole('option', { name: /^Write report/ })).toHaveAttribute(
    'aria-selected',
    'false',
  )
  await page.keyboard.press('Escape')
  await page.getByRole('tab', { name: '时间', exact: true }).click()
  await page.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(page.locator('.dag-output-settings')).toBeVisible()
  expect(f.requests.filter((r) => r.method === 'PUT')).toHaveLength(0)
  await selectChoice(page, page, '结果选择', '自动')
  await expect(page.locator('.dag-canvas-node').getByLabel('结果节点')).toHaveCount(1)
})

for (const count of [20, 100])
  test(`${count}-node graph has bounded viewport, readable nodes and an accessible table`, async ({
    page,
  }) => {
    const config = definition()
    config.dag.nodes = Array.from({ length: count }, (_, index) => ({
      ...structuredClone(config.dag.nodes[0]),
      id: `node-${index}`,
      name: `Research step ${index} 中文 long name`,
      pre: index > 1 ? [`node-${Math.floor((index - 1) / 2)}`] : [],
    }))
    await fixture(page, config)
    await page.goto(base + '?task=ux-fixture')
    await expect(page.locator('.dag-interactive-canvas')).toHaveAttribute(
      'data-layout-ready',
      'true',
    )
    await expect(page.locator('.react-flow__node')).toHaveCount(count)
    const scale = await page
      .locator('.react-flow__viewport')
      .evaluate((el) => Number(el.style.transform.match(/scale\(([^)]+)/)?.[1]))
    expect(scale).toBeGreaterThanOrEqual(0.65)
    await page.getByRole('button', { name: '表格', exact: true }).click()
    const list = page.getByRole('table', { name: '节点', exact: true })
    await expect(list.locator('.dag-list-node')).toHaveCount(count)
    await list.locator('.dag-list-node').first().focus()
    await page.keyboard.press('ArrowDown')
    await page.keyboard.press('Enter')
    await expect(page.getByLabel('名称', { exact: true })).toHaveValue(config.dag.nodes[1].name)
    await page.getByRole('button', { name: '关闭', exact: true }).click()
    await page.getByRole('button', { name: '搜索节点', exact: true }).click()
    await page.getByRole('textbox', { name: '搜索节点', exact: true }).fill(`step ${count - 1} `)
    await expect(list.locator('.dag-list-node')).toHaveCount(1)
    await page.keyboard.press('Escape')
    for (const width of [390, 1440, 2200]) {
      await page.setViewportSize({ width, height: 900 })
      await page.evaluate(
        () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))),
      )
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
        true,
      )
    }
  })

test('new DAG total limit is off, toggling preserves its duration draft without task writes', async ({
  page,
}) => {
  const f = await fixture(page)
  await page.goto(base)
  await page.getByRole('button', { name: '下一步', exact: true }).click()
  // The control shares the execution-policy table row and has an explicit accessible duration label.
  const toggle = page.getByRole('checkbox', { name: '启用', exact: true })
  await expect(toggle).not.toBeChecked()
  await expect(page.getByRole('textbox', { name: '整图总时限', exact: true })).toHaveCount(0)
  await toggle.check()
  await page.getByRole('textbox', { name: '整图总时限', exact: true }).fill('2h')
  await toggle.uncheck()
  await page.getByRole('tab', { name: '任务', exact: true }).click()
  await page.getByRole('tab', { name: '时间', exact: true }).click()
  await toggle.check()
  await expect(page.getByRole('textbox', { name: '整图总时限', exact: true })).toHaveValue('2h')
  expect(f.requests.filter((r) => r.path === '/tasks/validate' || r.method === 'PUT')).toHaveLength(
    0,
  )
})

test('DAG total time limit and enable control are localized in English', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('wakeit-language', 'en'))
  await fixture(page)
  await page.goto(base)
  await page.getByRole('button', { name: 'Next', exact: true }).click()
  const row = page.locator('[data-field="execution.timeout"]')
  await expect(row).toContainText('Graph total time limit')
  const enable = row.getByRole('checkbox', { name: 'Enable', exact: true })
  await expect(enable).not.toBeChecked()
  await enable.check()
  await expect(
    row.getByRole('textbox', { name: 'Graph total time limit', exact: true }),
  ).toHaveValue('1h')
})
