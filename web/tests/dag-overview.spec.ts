import { test, expect, type Page } from '@playwright/test'
import { definition, fixture, workspace } from './dag-fixture'

async function overview(page: Page, config = definition()) {
  const task = {
    id: config.id,
    config,
    revision: 7,
    next_at: null,
    last_run: null,
    archived_at: null,
  }
  const f = await fixture(page, config)
  await page.route('**/workspaces/*/tasks?*', (route) =>
    route.fulfill({ json: { items: [task], total: 1, page: 1, page_size: 20 } }),
  )
  await page.route('**/workspaces/*/runs?*', (route) =>
    route.fulfill({ json: { items: [], total: 0, page: 1, page_size: 20 } }),
  )
  await page.goto(`/w/${workspace}/tasks?task=${config.id}`)
  await expect(page.locator('.dag-graph-node')).toHaveCount(2)
  return f
}

test('Task overview uses the same three upper-right controls and fullscreen preserves selected node and graph scale', async ({
  page,
}) => {
  const f = await overview(page),
    graph = page.locator('.dag-structure'),
    controls = graph.getByRole('toolbar', { name: '画布操作', exact: true }),
    plane = graph.locator('.dag-graph-plane')
  await expect(controls.getByRole('button')).toHaveCount(3)
  for (const name of ['适应视图', '原始大小', '定位当前节点'])
    await expect(graph.getByRole('button', { name, exact: true })).toHaveCount(0)
  const frame = (await graph.locator('.dag-graph-frame').boundingBox())!,
    tools = (await controls.boundingBox())!,
    height = (await graph.boundingBox())!.height
  expect(tools.y - frame.y).toBe(16)
  expect(frame.x + frame.width - tools.x - tools.width).toBe(16)
  await controls.getByRole('button', { name: '放大', exact: true }).click()
  const scale = await plane.evaluate((el) => el.style.transform)
  await graph.getByRole('button', { name: 'Write report', exact: true }).click()
  await controls.getByRole('button', { name: '全屏', exact: true }).click()
  const full = graph.locator('.dag-structure-fullscreen')
  await expect(full).toBeVisible()
  const expanded = (await full.boundingBox())!
  expect(expanded.x).toBe(0)
  expect(expanded.y).toBe(44)
  expect(expanded.width).toBe(1440)
  await expect(graph.getByRole('button', { name: 'Write report', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  )
  await page.mouse.move(600, 400)
  await page.keyboard.press('Escape')
  await expect(full).toHaveCount(0)
  expect((await graph.boundingBox())!.height).toBe(height)
  expect(await plane.evaluate((el) => el.style.transform)).toBe(scale)
  await expect(page.locator('.task-overview-prompt-head')).toContainText('Write report')
  expect(f.requests.filter((r) => r.method !== 'GET')).toHaveLength(0)
})

test('Task overview fullscreen exits on view change and controls fit both themes, languages and narrow screens', async ({
  page,
}) => {
  await overview(page)
  for (const [width, language, theme] of [
    [1440, 'zh', 'light'],
    [390, 'zh', 'light'],
    [1440, 'en', 'dark'],
  ] as const) {
    await page.setViewportSize({ width, height: 900 })
    await page.evaluate(
      async ({ language, theme }) => {
        const i18n = await import('/src/i18n.ts')
        await i18n.setLanguage(language)
        document.documentElement.dataset.theme = theme
      },
      { language, theme },
    )
    await page.evaluate(
      () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))),
    )
    const graph = page.locator('.dag-structure'),
      controls = graph.getByRole('toolbar', {
        name: language === 'zh' ? '画布操作' : 'Canvas controls',
        exact: true,
      })
    await expect(controls.getByRole('button')).toHaveCount(3)
    const output = graph.getByRole('button', { name: 'Write report', exact: true })
    await expect(output.locator('.dag-node-card-header')).toContainText('Preview device')
    await expect(output.locator('.dag-node-card-agent')).toContainText('pi')
    await expect(output.locator('.dag-output-badge')).toHaveText(
      language === 'zh' ? '输出' : 'Output',
    )
    await expect(output).toHaveAccessibleDescription(language === 'zh' ? '输出节点' : 'Output node')
    await expect(output).toHaveCSS('height', '96px')
    await controls
      .getByRole('button', { name: language === 'zh' ? '全屏' : 'Full screen', exact: true })
      .click()
    const full = graph.locator('.dag-structure-fullscreen')
    await expect(full).toBeVisible()
    expect((await full.boundingBox())!.width).toBe(width)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/dag-overview-fullscreen-${width}-${language}-${theme}.png`,
    })
    await graph
      .getByRole('button', { name: language === 'zh' ? '表格' : 'Table', exact: true })
      .click()
    await expect(full).toHaveCount(0)
    await expect(graph.getByRole('table')).toBeVisible()
    await expect(graph.getByRole('table').locator('.dag-output-badge')).toHaveText(
      language === 'zh' ? '输出' : 'Output',
    )
    await graph
      .getByRole('button', { name: language === 'zh' ? '结构图' : 'Graph', exact: true })
      .click()
    await expect(controls).toBeVisible()
  }
})

test('long node and environment names retain full identity without covering the output badge', async ({
  page,
}) => {
  const config = definition()
  const runner = 'Preview workstation with a deliberately long device name'
  const agent = 'ExampleAgentWithALongName'
  for (const node of config.dag.nodes) {
    node.name += '：整理跨设备资料并生成最终报告 Long bilingual node name'
    node.runner = runner
    node.agent = agent
  }
  await overview(page, config)
  const graph = page.locator('.dag-structure')
  await graph.getByRole('button', { name: '全屏', exact: true }).click()
  const output = graph.getByRole('button', { name: config.dag.nodes[1].name, exact: true })
  await expect(output).toHaveCSS('width', '240px')
  await expect(output).toHaveCSS('height', '96px')
  await expect(output.locator('.dag-node-card-name strong')).toHaveAttribute(
    'title',
    config.dag.nodes[1].name,
  )
  await output.locator('.dag-node-card-runner').hover()
  await expect(page.getByRole('tooltip')).toHaveText(runner)
  await page.mouse.move(600, 600, { steps: 8 })
  await expect(page.getByRole('tooltip')).toHaveCount(0)
  await output.locator('.dag-node-card-agent').hover()
  await expect(page.getByRole('tooltip')).toHaveText(agent)
  const bounds = await output.evaluate((el) => {
    const card = el.getBoundingClientRect()
    return Array.from(el.children).map((child) => {
      const rect = child.getBoundingClientRect()
      return (
        rect.left >= card.left &&
        rect.right <= card.right &&
        rect.top >= card.top &&
        rect.bottom <= card.bottom
      )
    })
  })
  expect(bounds.every(Boolean)).toBe(true)
  await page.mouse.move(600, 600, { steps: 8 })
  await page.screenshot({ path: test.info().outputPath('dag-node-long-environment.png') })
})
