import { test, expect, type Page, type Locator } from '@playwright/test'
import { base, definition, fixture, editNode, switchNode, runnerId } from './dag-fixture'
const position = (element: HTMLElement) => {
  const m = element.style.transform.match(/translate\(([^p]+)px,\s*([^p]+)px\)/)!
  return { x: Number(m[1]), y: Number(m[2]) }
}
async function point(locator: Locator) {
  const b = (await locator.boundingBox())!
  return { x: b.x + b.width / 2, y: b.y + b.height / 2 }
}
async function drag(
  page: Page,
  from: Locator,
  to: { x: number; y: number },
  button: 'left' | 'middle' = 'left',
) {
  await page.mouse.move(...(Object.values(await point(from)) as [number, number]))
  await page.mouse.down({ button })
  await page.mouse.move(to.x, to.y, { steps: 12 })
  await page.mouse.up({ button })
}
async function graph(page: Page, plain = false) {
  const config = definition()
  if (plain) config.dag.nodes[1].prompt = 'Write independently'
  const f = await fixture(page, config)
  f.setLayout({ version: 1, positions: { source: { x: 40, y: 120 }, report: { x: 420, y: 120 } } })
  await page.goto(base + '?task=ux-fixture')
  await expect(page.locator('.dag-interactive-canvas')).toHaveAttribute('data-layout-ready', 'true')
  return f
}
async function snapshot(page: Page) {
  await page.route('**/tasks/ux-fixture', async (route) =>
    route.request().method() === 'PUT'
      ? route.fulfill({ status: 503, json: { error: 'Captured draft' } })
      : route.fallback(),
  )
  await page.getByRole('tab', { name: '时间', exact: true }).click()
  const request = page.waitForRequest(
    (r) => r.url().endsWith('/tasks/ux-fixture') && r.method() === 'PUT',
  )
  await page.getByRole('button', { name: '保存任务', exact: true }).click()
  const body = (await request).postDataJSON()
  await expect(page.getByRole('alert')).toContainText('Captured draft')
  return body
}

test('pointer drop uses flow coordinates after pan and zoom, keyboard add remains available', async ({
  page,
}) => {
  await fixture(page)
  await page.goto(base)
  const canvas = page.locator('.dag-interactive-canvas')
  await expect(canvas).toHaveAttribute('data-layout-ready', 'true')
  await page.getByRole('button', { name: '放大', exact: true }).click()
  const box = (await canvas.boundingBox())!
  await page.mouse.move(box.x + 100, box.y + 100)
  await page.mouse.down({ button: 'middle' })
  await page.mouse.move(box.x + 200, box.y + 150, { steps: 8 })
  await page.mouse.up({ button: 'middle' })
  const drop = { x: box.x + 320, y: box.y + 260 }
  const viewport = await page.locator('.react-flow__viewport').evaluate((el) => ({
    x: el.getBoundingClientRect().left,
    y: el.getBoundingClientRect().top,
    scale: Number(el.style.transform.match(/scale\(([^)]+)/)?.[1]),
  }))
  await drag(page, page.getByRole('button', { name: 'Preview device 上的 Pi', exact: true }), drop)
  await expect(page.locator('.react-flow__node')).toHaveCount(1)
  const actual = await page.locator('.react-flow__node').first().evaluate(position)
  expect(actual.x).toBeCloseTo((drop.x - viewport.x) / viewport.scale - 120, 0)
  expect(actual.y).toBeCloseTo((drop.y - viewport.y) / viewport.scale - 48, 0)
  await expect(page.getByLabel('名称', { exact: true })).toHaveValue('Pi 1')
  await expect(page.getByRole('combobox', { name: 'Runner', exact: true })).toContainText(
    'Preview device',
  )
  await expect(page.getByRole('combobox', { name: 'Agent', exact: true })).toContainText('Pi')
  await page.getByRole('button', { name: '关闭', exact: true }).click()
  await page.getByRole('button', { name: 'Preview device 上的 Pi', exact: true }).focus()
  await page.keyboard.press('Enter')
  await expect(page.getByLabel('名称', { exact: true })).toHaveValue('Pi 2')
})

test('Agent palette filters detection and revocation, retains offline devices, searches IDs and cancels drops', async ({
  page,
}) => {
  await fixture(page)
  await page.route('**/runners', (route) =>
    route.fulfill({
      json: [
        {
          id: runnerId,
          name: 'Offline lab',
          online: false,
          revoked: false,
          agents: [
            { id: 'pi', name: 'Pi', available: true, parameters: {} },
            { id: 'missing-agent', name: 'Missing', available: false, parameters: {} },
          ],
        },
        {
          id: 'revoked',
          name: 'Retired',
          online: true,
          revoked: true,
          agents: [{ id: 'codex', name: 'Codex', available: true, parameters: {} }],
        },
      ],
    }),
  )
  await page.goto(base)
  await expect(page.locator('.dag-agent-entry')).toHaveCount(1)
  await expect(page.locator('.dag-palette-runner')).toContainText('离线')
  await page.getByLabel('搜索 Agent / 设备', { exact: true }).fill('missing-agent')
  await expect(page.locator('.dag-agent-entry')).toHaveCount(0)
  await page.getByLabel('搜索 Agent / 设备', { exact: true }).fill('pi')
  const entry = page.locator('.dag-agent-entry')
  await page.mouse.move(...(Object.values(await point(entry)) as [number, number]))
  await page.mouse.down()
  await page.mouse.move(600, 400, { steps: 10 })
  await expect(page.locator('.dag-agent-ghost')).toBeVisible()
  await page.keyboard.press('Escape')
  await page.mouse.up()
  await expect(page.locator('.react-flow__node')).toHaveCount(0)
  await entry.click()
  await expect(page.getByLabel('名称', { exact: true })).toHaveValue('Pi 1')
})

test('connections reject cycles, reconnect targets and undo the topology without reverting later text', async ({
  page,
}) => {
  const f = await graph(page, true)
  const source = page.locator('.react-flow__node[data-id="source"]'),
    report = page.locator('.react-flow__node[data-id="report"]')
  await drag(page, report.locator('.source'), await point(source.locator('.target')))
  await expect(page.getByRole('alert')).toContainText('环')
  await drag(page, source.locator('.source'), await point(source.locator('.target')))
  await expect(page.getByRole('alert')).toContainText('自身')
  await drag(page, source.locator('.source'), await point(report.locator('.target')))
  await expect(page.getByRole('alert')).toContainText('已存在')
  await expect(page.locator('.react-flow__edge')).toHaveCount(1)
  // Create a separate root, then reconnect the existing dependency to it.
  await page.getByRole('button', { name: 'Preview device 上的 Pi', exact: true }).click()
  await page.getByLabel('指令', { exact: true }).fill('Third independent output')
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.getByRole('dialog').getByRole('button', { name: '选择文件夹', exact: true }).click()
  await page.getByRole('button', { name: '关闭', exact: true }).click()
  const third = page.locator('.react-flow__node').last()
  // Move the new root clear of the edge before reconnecting.
  const b = (await third.boundingBox())!
  await drag(page, third, { x: b.x + b.width / 2, y: b.y + b.height / 2 + 150 })
  await drag(
    page,
    page.locator('.react-flow__edgeupdater-target'),
    await point(third.locator('.target')),
  )
  await third.click()
  await expect(page.getByRole('combobox', { name: '前置节点', exact: true })).toContainText(
    'Collect sources',
  )
  await page.getByLabel('指令', { exact: true }).fill('Text changed after reconnect')
  await page.getByRole('button', { name: '撤销', exact: true }).click()
  await expect(page.getByRole('combobox', { name: '前置节点', exact: true })).toContainText(
    '无前置节点',
  )
  await expect(page.getByLabel('指令', { exact: true })).toHaveValue('Text changed after reconnect')
  await page.getByRole('button', { name: '重做', exact: true }).click()
  const body = await snapshot(page)
  const added = body.config.dag.nodes[2]
  expect(added.pre).toEqual(['source'])
  expect(body.config.dag.nodes[1].pre).toEqual([])
  expect(body.expected_revision).toBe(7)
  expect(f.requests.filter((r) => r.method === 'PUT')).toHaveLength(0) // intercepted failed save
})

test('node drag is one history step; multi-selection moves together and a selected edge can be deleted', async ({
  page,
}) => {
  await graph(page, true)
  const source = page.locator('.react-flow__node[data-id="source"]'),
    report = page.locator('.react-flow__node[data-id="report"]')
  const left = (await source.boundingBox())!,
    right = (await report.boundingBox())!
  await page.mouse.move(left.x - 16, left.y - 16)
  await page.mouse.down()
  await page.mouse.move(right.x + right.width + 16, right.y + right.height + 16, { steps: 12 })
  await page.mouse.up()
  await expect(page.locator('.react-flow__node.selected')).toHaveCount(2)
  await page.locator('.react-flow__pane').click({ position: { x: 20, y: 20 } })
  const before = await source.evaluate(position)
  const b = await point(source)
  await drag(page, source, { x: b.x + 50, y: b.y + 70 })
  await expect.poll(() => source.evaluate(position)).not.toEqual(before)
  await page.getByRole('button', { name: '撤销', exact: true }).click()
  await expect.poll(() => source.evaluate(position)).toEqual(before)
  await page.locator('.react-flow__pane').click({ position: { x: 20, y: 20 } })
  await page.keyboard.down('Shift')
  await source.click()
  await report.click()
  await page.keyboard.up('Shift')
  await expect(page.locator('.react-flow__node.selected')).toHaveCount(2)
  const a = await source.evaluate(position),
    c = await report.evaluate(position),
    p = await point(source)
  await drag(page, source, { x: p.x + 35, y: p.y + 45 })
  const an = await source.evaluate(position),
    cn = await report.evaluate(position)
  expect(an.x - a.x).toBeCloseTo(cn.x - c.x, 1)
  expect(an.y - a.y).toBeCloseTo(cn.y - c.y, 1)
  await page.getByRole('button', { name: '撤销', exact: true }).click()
  await expect.poll(() => source.evaluate(position)).toEqual(a)
  await expect.poll(() => report.evaluate(position)).toEqual(c)
  // Escape and a blank click clear the inspector; select only the edge for Delete.
  await page.keyboard.press('Escape')
  await page.locator('.react-flow__pane').click({ position: { x: 20, y: 20 } })
  await page.locator('.react-flow__edge').focus()
  await page.keyboard.press('Enter')
  await page.keyboard.press('Delete')
  await expect(page.locator('.react-flow__edge')).toHaveCount(0)
  await expect(page.locator('.react-flow__node')).toHaveCount(2)
  await page.getByRole('button', { name: '撤销', exact: true }).click()
  await expect(page.locator('.react-flow__edge')).toHaveCount(1)
})

test('floating panel resizes, prompt expands within the workbench, Escape closes the top layer and caret survives switches', async ({
  page,
}) => {
  await graph(page)
  await editNode(page)
  const panel = page.locator('.dag-floating-panel'),
    resizer = page.getByRole('separator', { name: '调整节点配置宽度', exact: true })
  await resizer.focus()
  await page.keyboard.press('ArrowLeft')
  expect((await panel.boundingBox())!.width).toBe(530)
  const text = page.getByLabel('指令', { exact: true })
  await text.focus()
  await text.evaluate((el: HTMLTextAreaElement) => {
    el.setSelectionRange(5, 9)
    el.dispatchEvent(new Event('select', { bubbles: true }))
  })
  await switchNode(page, 'Write report')
  await switchNode(page, 'Collect sources')
  expect(
    await text.evaluate((el: HTMLTextAreaElement) => [el.selectionStart, el.selectionEnd]),
  ).toEqual([5, 9])
  await page.getByRole('button', { name: '展开编辑指令', exact: true }).click()
  await expect(page.locator('.dag-prompt-focus')).toBeVisible()
  const area = (await page.locator('.dag-canvas-body').boundingBox())!,
    focus = (await page.locator('.dag-prompt-focus').boundingBox())!
  expect(focus.y).toBeGreaterThanOrEqual(area.y)
  expect(focus.y + focus.height).toBeLessThanOrEqual(area.y + area.height)
  await page.getByRole('dialog').getByLabel('指令', { exact: true }).fill('Expanded prompt\n中文')
  await page.keyboard.press('Escape')
  await expect(page.locator('.dag-floating-panel')).toBeVisible()
  await expect(text).toHaveValue('Expanded prompt\n中文')
  await page.getByRole('combobox', { name: 'Runner', exact: true }).click()
  await page.keyboard.press('Escape')
  await expect(panel).toBeVisible()
  await expect(page.getByRole('option')).toHaveCount(0)
  // Middle-button panning must not dismiss the panel.
  const pane = page.locator('.react-flow__pane'),
    p = (await pane.boundingBox())!
  await page.mouse.move(p.x + 30, p.y + 30)
  await page.mouse.down({ button: 'middle' })
  await page.mouse.move(p.x + 60, p.y + 50)
  await page.mouse.up({ button: 'middle' })
  await expect(panel).toBeVisible()
  await pane.click({ position: { x: 40, y: 40 } })
  await expect(panel).toHaveCount(0)
})

test('history patches, successor inheritance and cyclic reconnect validation are independent of presentation', async ({
  page,
}) => {
  await graph(page)
  const result = await page.evaluate(async () => {
    const { GraphHistory, createAgentNode, connectionIssue, connect, removeNodes, positionOf } =
      await import('/src/dag/workbench.ts')
    const a: any = {
      id: 'a',
      name: 'Pi 1',
      pre: [],
      runner: 'r',
      agent: 'pi',
      cwd: '/workspace',
      parameters: { model: 'a' },
      prompt: 'original',
      session: { id: 'kept' },
      timeout: '2h',
    }
    const b: any = { ...structuredClone(a), id: 'b', name: 'B', pre: ['a'] }
    const before: any = {
      dag: { nodes: [a, b], results: ['b'] },
      layout: { version: 1, positions: { a: { x: 0, y: 0 }, b: { x: 320, y: 0 } } },
    }
    const history = new GraphHistory(),
      moved = structuredClone(before)
    moved.layout.positions.a.x = 40
    history.record(before, moved)
    moved.dag.nodes[1].prompt = 'later text'
    const undone = history.undo(moved),
      redone = history.redo(undone)
    const choice: any = {
      runner: { id: 'r' },
      agent: { id: 'pi', name: 'Pi', parameters: { model: { enum: ['a'] } } },
    }
    const successor = createAgentNode([a, b], choice, a),
      cross = createAgentNode([a, b], { ...choice, runner: { id: 'other' } }, a)
    const removal = removeNodes(before, ['b'])
    return {
      undone,
      redone,
      successor,
      cross,
      removal,
      inheritedPosition: positionOf({ version: 1, positions: {} }, 'constructor'),
      cycle: connectionIssue(before.dag, 'b', 'a'),
      duplicate: connectionIssue(before.dag, 'a', 'b'),
      reconnect: connectionIssue(before.dag, 'b', 'a', { source: 'a', target: 'b' }),
      connected: connect(before.dag, 'b', 'a', { source: 'a', target: 'b' }),
    }
  })
  expect(result.undone.layout.positions.a.x).toBe(0)
  expect(result.undone.dag.nodes[1].prompt).toBe('later text')
  expect(result.redone.layout.positions.a.x).toBe(40)
  expect(result.successor.name).toBe('Pi 2')
  expect(result.successor.pre).toEqual(['a'])
  expect(result.successor.session).toBeNull()
  expect(result.successor.parameters.model).toBe('a')
  expect(result.successor.timeout).toBe('2h')
  expect(result.cross.cwd).toBe('')
  expect(result.cross.parameters).toEqual({})
  expect(result.cross.timeout).toBe('1h')
  expect(result.cycle).toBe('cycle')
  expect(result.duplicate).toBe('duplicate')
  expect(result.reconnect).toBeNull()
  expect(result.removal.dag.results).toEqual(['b'])
  expect(result.removal.layout.positions.b).toBeUndefined()
  expect(result.inheritedPosition).toBeUndefined()
})

test('ports add an AND predecessor and drag-to-empty only creates a successor after Agent selection', async ({
  page,
}) => {
  await graph(page, true)
  await page.getByRole('button', { name: 'Preview device 上的 Pi', exact: true }).click()
  await page.getByRole('button', { name: '关闭', exact: true }).click()
  await page.getByRole('button', { name: '整理布局', exact: true }).click()
  await expect(page.getByRole('button', { name: '整理布局', exact: true })).toBeEnabled()
  await page.evaluate(
    () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))),
  )
  const source = page.locator('.react-flow__node[data-id="source"]'),
    report = page.locator('.react-flow__node[data-id="report"]'),
    third = page.locator('.react-flow__node').last()
  await drag(page, source.locator('.source'), await point(third.locator('.target')))
  await third.click()
  await expect(page.getByRole('combobox', { name: '前置节点', exact: true })).toContainText(
    'Collect sources',
  )
  await page.getByRole('button', { name: '关闭', exact: true }).click()
  await page.getByRole('button', { name: '整理布局', exact: true }).click()
  await expect(page.getByRole('button', { name: '整理布局', exact: true })).toBeEnabled()
  await page.evaluate(
    () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))),
  )
  const b = (await page.locator('.dag-interactive-canvas').boundingBox())!,
    blank = { x: b.x + b.width - 80, y: b.y + b.height - 150 }
  await drag(page, report.locator('.source'), blank)
  await expect(page.getByRole('dialog')).toBeVisible()
  await expect(page.locator('.react-flow__node')).toHaveCount(3)
  await page.getByRole('dialog').getByRole('button', { name: '关闭', exact: true }).click()
  await expect(page.locator('.react-flow__node')).toHaveCount(3)
  await drag(page, report.locator('.source'), blank)
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Preview device 上的 Pi', exact: true })
    .click()
  await expect(page.locator('.react-flow__node')).toHaveCount(4)
  await expect(page.getByRole('combobox', { name: '前置节点', exact: true })).toContainText(
    'Write report',
  )
  await expect(page.getByLabel('指令', { exact: true })).toHaveValue('')
})

test('touch pointer drag creates a node through the same Agent source', async ({ browser }) => {
  const context = await browser.newContext({
    hasTouch: true,
    isMobile: true,
    viewport: { width: 1440, height: 900 },
    locale: 'zh-CN',
    baseURL: process.env.WAKEIT_TEST_URL ?? 'http://127.0.0.1:58631',
  })
  const page = await context.newPage()
  try {
    await fixture(page)
    await page.goto(base)
    const p = await point(
        page.getByRole('button', { name: 'Preview device 上的 Pi', exact: true }),
      ),
      box = (await page.locator('.dag-interactive-canvas').boundingBox())!,
      target = { x: box.x + 260, y: box.y + 260 }
    const session = await context.newCDPSession(page)
    await session.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [p] })
    for (let i = 1; i <= 8; i++)
      await session.send('Input.dispatchTouchEvent', {
        type: 'touchMove',
        touchPoints: [{ x: p.x + ((target.x - p.x) * i) / 8, y: p.y + ((target.y - p.y) * i) / 8 }],
      })
    await session.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] })
    await expect(page.locator('.react-flow__node')).toHaveCount(1)
    await expect(page.getByLabel('名称', { exact: true })).toHaveValue('Pi 1')
  } finally {
    await context.close()
  }
})

test('canvas controls sit at the upper right, maximize the editor and leave draft and dropdown behavior intact', async ({
  page,
}) => {
  const f = await graph(page)
  const controls = page.getByRole('toolbar', { name: '画布操作', exact: true }),
    canvas = page.locator('.dag-interactive-canvas')
  await expect(controls.getByRole('button')).toHaveCount(3)
  await expect(page.getByRole('button', { name: '适应视图', exact: true })).toHaveCount(0)
  await expect(page.getByRole('button', { name: '原始大小', exact: true })).toHaveCount(0)
  const box = (await controls.boundingBox())!,
    area = (await canvas.boundingBox())!
  expect(box.y - area.y).toBe(16)
  expect(area.x + area.width - box.x - box.width).toBe(16)
  const before = (await page.locator('.dag-editor').boundingBox())!
  await controls.getByRole('button', { name: '全屏', exact: true }).click()
  await expect(page.locator('.dag-editor')).toHaveClass(/is-fullscreen/)
  const expanded = (await page.locator('.dag-editor').boundingBox())!
  expect(expanded.x).toBe(0)
  expect(expanded.width).toBe(1440)
  expect(expanded.width).toBeGreaterThan(before.width)
  await expect(page.getByLabel('任务名称', { exact: true })).toBeVisible()
  await editNode(page)
  const floating = (await page.locator('.dag-floating-panel').boundingBox())!,
    actions = (await controls.boundingBox())!
  expect(actions.x + actions.width).toBeLessThanOrEqual(floating.x - 16)
  await page.getByRole('combobox', { name: 'Runner', exact: true }).click()
  await expect(page.getByRole('option', { name: /Preview device/ })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.locator('.dag-editor')).toHaveClass(/is-fullscreen/)
  await page.keyboard.press('Escape')
  await expect(page.locator('.dag-floating-panel')).toHaveCount(0)
  await page.keyboard.press('Escape')
  await expect(page.locator('.dag-editor')).not.toHaveClass(/is-fullscreen/)
  await controls.getByRole('button', { name: '全屏', exact: true }).click()
  await page.getByRole('tab', { name: '时间', exact: true }).click()
  await expect(page.locator('.dag-editor')).not.toHaveClass(/is-fullscreen/)
  expect(f.requests.filter((r) => r.method === 'PUT')).toHaveLength(0)
})
