import { test, expect } from '@playwright/test'
import { selectChoice } from './choices'
import { base, runnerId, definition, fixture, editNode, switchNode } from './dag-fixture'
test('floating editor and table share identity and column geometry without changing canvas size', async ({
  page,
}) => {
  await fixture(page)
  await page.goto(base + '?task=ux-fixture')
  await expect(page.locator('.dag-floating-panel')).toHaveCount(0)
  await expect(page.locator('.react-flow__node').first()).toContainText('Preview device')
  const output = page.locator('.dag-canvas-node').filter({ hasText: 'Write report' })
  await expect(output.locator('.dag-node-card-header')).toContainText('Preview device')
  await expect(output.locator('.dag-node-card-header')).toContainText('Pi')
  await expect(output.locator('.dag-node-card-footer').getByLabel('输出节点')).toHaveText('输出')
  await expect(output).toHaveCSS('width', '240px')
  await expect(output).toHaveCSS('height', '96px')
  const before = (await page.locator('.dag-interactive-canvas').boundingBox())!
  await editNode(page)
  const panel = (await page.locator('.dag-floating-panel').boundingBox())!
  expect(panel.width).toBe(520)
  expect((await page.locator('.dag-interactive-canvas').boundingBox())!.width).toBe(before.width)
  await page.getByRole('button', { name: '表格', exact: true }).click()
  await expect(page.locator('.dag-expanded-node')).toHaveCount(0)
  await expect(page.locator('.dag-node-table .dag-table-head')).toHaveCount(1)
  const deltas = await page
    .locator('.dag-list-node')
    .first()
    .evaluate((el) =>
      Array.from(el.children)
        .slice(0, -1)
        .map((cell, i) =>
          Math.abs(
            cell.getBoundingClientRect().x -
              el
                .closest('[role=table]')!
                .querySelector('.dag-table-head')!
                .children[i].getBoundingClientRect().x,
          ),
        ),
    )
  expect(Math.max(...deltas)).toBeLessThan(1)
  await page.getByRole('button', { name: '关闭', exact: true }).click()
  for (const [width, language, theme] of [
    [1440, 'zh', 'light'],
    [390, 'zh', 'light'],
    [1440, 'en', 'dark'],
    [2200, 'en', 'light'],
  ] as const) {
    await page.setViewportSize({ width, height: 900 })
    await page.evaluate(
      async ({ language, theme }) => {
        const module = await import('/src/i18n.ts')
        await module.setLanguage(language)
        document.documentElement.dataset.theme = theme
      },
      { language, theme },
    )
    const badge = page.locator('.dag-node-table .dag-output-badge')
    await expect(badge).toHaveText(language === 'zh' ? '输出' : 'Output')
    await expect(badge).toHaveCSS(
      'color',
      theme === 'dark' ? 'rgb(88, 166, 255)' : 'rgb(9, 105, 218)',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/dag-canvas-table-${width}-${language}-${theme}.png`,
    })
  }
})
test('successor and duplicate keep explicit scope; complete saves preserve revision and paused state', async ({
  page,
}) => {
  const f = await fixture(page)
  f.config.dag.nodes[0].session = { id: 'original-session' }
  await page.goto(base + '?task=ux-fixture')
  await editNode(page)
  await page
    .locator('.dag-floating-header')
    .getByRole('button', { name: '节点操作', exact: true })
    .click()
  await page.getByRole('menuitem', { name: '复制节点', exact: true }).click()
  await expect(page.getByLabel('名称', { exact: true })).toHaveValue(/Collect sources/)
  await expect(page.getByLabel('名称', { exact: true })).toBeFocused()
  await page
    .locator('.dag-floating-header')
    .getByRole('button', { name: '节点操作', exact: true })
    .click()
  await page.getByRole('menuitem', { name: '新增后续节点', exact: true }).click()
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Preview device 上的 Pi', exact: true })
    .click()
  await expect(page.getByLabel('名称', { exact: true })).toBeFocused()
  await expect(page.getByLabel('指令', { exact: true })).toHaveValue('')
  await page.getByLabel('指令', { exact: true }).fill('Follow up')
  await page.getByRole('tab', { name: '时间', exact: true }).click()
  await selectChoice(page, page, '类型', '手动执行')
  await page.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect.poll(() => f.requests.filter((r) => r.method === 'PUT').length).toBe(1)
  const body = f.requests.find((r) => r.method === 'PUT')!.body
  expect(body.expected_revision).toBe(7)
  expect(body.config.enabled).toBe(false)
  expect(body.config.schedule).toBeNull()
  const copy = body.config.dag.nodes[2],
    next = body.config.dag.nodes[3]
  expect(copy.session).toEqual({ id: 'original-session' })
  expect(copy.pre).toEqual([])
  expect(copy.id).not.toBe('source')
  expect(next.pre).toEqual([copy.id])
  expect(next.session).toBeNull()
  expect(next.runner).toBe(runnerId)
  expect(next.parameters.model).toBe('model-a')
})
test('node UI state, model mode, Session drafts and graph scale survive node and step changes', async ({
  page,
}) => {
  await fixture(page)
  await page.goto(base + '?task=ux-fixture')
  await page.getByRole('button', { name: '放大', exact: true }).click()

  await editNode(page)
  await selectChoice(page, page, '模型', '自定义模型 ID')
  await page.getByLabel('模型', { exact: true }).fill('private-model')
  await page.locator('.dag-advanced > summary').click()
  await page.getByRole('combobox', { name: '会话', exact: true }).click()
  await page.getByRole('button', { name: '输入会话 ID…', exact: true }).click()
  await page.getByLabel('会话 ID', { exact: true }).fill('unfinished-session-id')
  await page.keyboard.press('Escape')
  await switchNode(page, 'Write report')
  await switchNode(page, 'Collect sources')
  await expect(page.locator('.dag-advanced')).toHaveAttribute('open', '')
  await expect(page.getByLabel('模型', { exact: true })).toHaveValue('private-model')
  await page.getByRole('combobox', { name: '会话', exact: true }).click()
  await page.getByRole('button', { name: '输入会话 ID…', exact: true }).click()
  await expect(page.getByLabel('会话 ID', { exact: true })).toHaveValue('unfinished-session-id')
  await page.keyboard.press('Escape')
  const pane = page.locator('.dag-floating-body')
  await pane.evaluate((el) => {
    el.scrollTop = 300
  })
  const transform = await page.locator('.react-flow__viewport').evaluate((el) => el.style.transform)
  const storedScroll = await pane.evaluate((el) => el.scrollTop)
  expect(storedScroll).toBeGreaterThan(0)
  await page.getByRole('tab', { name: '时间', exact: true }).click()
  await page.getByRole('tab', { name: '任务', exact: true }).click()
  await expect.poll(() => pane.evaluate((el) => el.scrollTop)).toBe(storedScroll)
  expect(await page.locator('.react-flow__viewport').evaluate((el) => el.style.transform)).toBe(
    transform,
  )
  await expect(page.getByLabel('模型', { exact: true })).toHaveValue('private-model')
})
test('cycles are unavailable; deleting referenced nodes preserves a repairable draft and can be undone', async ({
  page,
}) => {
  const f = await fixture(page)
  await page.goto(base + '?task=ux-fixture')
  await editNode(page)
  await page.getByRole('combobox', { name: '前置节点', exact: true }).click()
  await expect(page.getByRole('option', { name: /^Write report/ })).toBeDisabled()
  await page.keyboard.press('Escape')
  await page
    .locator('.dag-floating-header')
    .getByRole('button', { name: '节点操作', exact: true })
    .click()
  await page.getByRole('menuitem', { name: '删除节点', exact: true }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('table')).toContainText('指令')
  await dialog.getByRole('button', { name: '删除节点', exact: true }).click()
  await expect(page.locator('.react-flow__node')).toHaveCount(1)
  await page.getByRole('button', { name: '下一步', exact: true }).click()
  await page.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(page.getByLabel('指令', { exact: true })).toHaveValue('{{ nodes.source.result }}')
  await expect(page.getByLabel('指令', { exact: true })).toHaveAttribute('aria-invalid', 'true')
  expect(f.requests.filter((r) => r.method === 'PUT')).toHaveLength(0)
  await page.getByRole('button', { name: '撤销', exact: true }).click()
  await expect(page.locator('.react-flow__node')).toHaveCount(2)
  await expect(page.getByLabel('指令', { exact: true })).toHaveAttribute('aria-invalid', 'false')
})
test('scheduling preserves every type, debounces incomplete input and converts selected timezones', async ({
  page,
}) => {
  const { requests } = await fixture(page)
  await page.goto(base + '?task=ux-fixture')
  await page.getByRole('tab', { name: '时间', exact: true }).click()
  const cron = page.getByLabel('Cron 表达式', { exact: true })
  await cron.fill('30 11 * * *')
  await selectChoice(page, page, '类型', '固定间隔')
  await page.getByLabel('间隔', { exact: true }).fill('3h')
  await selectChoice(page, page, '类型', '一次性执行')
  await page.getByLabel('执行时间', { exact: true }).fill('2026-10-21T10:15')
  await selectChoice(page, page, '类型', 'Cron')
  await expect(cron).toHaveValue('30 11 * * *')
  await selectChoice(page, page, '类型', '固定间隔')
  await expect(page.getByLabel('间隔', { exact: true })).toHaveValue('3h')
  await selectChoice(page, page, '类型', '一次性执行')
  await expect(page.getByLabel('执行时间', { exact: true })).toHaveValue('2026-10-21T10:15')
  await expect
    .poll(
      () =>
        requests.filter(
          (r) => r.path === '/schedule/preview' && r.body?.at === '2026-10-21T02:15:00.000Z',
        ).length,
    )
    .toBeGreaterThan(0)
  await selectChoice(page, page, '类型', 'Cron')
  const before = requests.filter((r) => r.path === '/schedule/preview').length
  await cron.fill('30 11')
  await page.waitForTimeout(650)
  expect(requests.filter((r) => r.path === '/schedule/preview').length).toBe(before)
  await expect(page.getByRole('alert')).toHaveCount(0)
  await cron.blur()
  await expect(page.getByRole('alert')).toContainText('五段')
})
test('IANA conversion rejects a missing local time and selects the earlier repeated occurrence', async ({
  page,
}) => {
  await fixture(page)
  await page.goto(base + '?task=ux-fixture')
  const result = await page.evaluate(async () => {
    const { utcDateTime, localDateTime } = await import('/src/dag/schedule.ts')
    let missing = false
    try {
      utcDateTime('2026-03-08T02:30:00', 'America/New_York')
    } catch {
      missing = true
    }
    return {
      missing,
      fold: utcDateTime('2026-11-01T01:30:00', 'America/New_York'),
      shanghai: utcDateTime('2026-10-20T09:00:00', 'Asia/Shanghai'),
      kathmandu: utcDateTime('2026-10-20T09:00:00', 'Asia/Kathmandu'),
      roundtrip: localDateTime('2026-11-01T05:30:00.000Z', 'America/New_York'),
    }
  })
  expect(result).toEqual({
    missing: true,
    fold: '2026-11-01T05:30:00.000Z',
    shanghai: '2026-10-20T01:00:00.000Z',
    kathmandu: '2026-10-20T03:15:00.000Z',
    roundtrip: '2026-11-01T01:30:00',
  })
})
test('Save marks and locates fields by stable paths; corrected errors disappear without losing the draft', async ({
  page,
}) => {
  const config = definition()
  config.dag.nodes[1].prompt = ''
  config.dag.nodes[0].timeout = 'bad'
  const { requests } = await fixture(page, config)
  await page.goto(base + '?task=ux-fixture')
  await page.getByRole('tab', { name: '时间', exact: true }).click()
  await page.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(page.getByLabel('指令', { exact: true })).toBeFocused()
  await expect(page.getByLabel('指令', { exact: true })).toHaveAttribute('aria-invalid', 'true')
  await page.getByLabel('指令', { exact: true }).fill('Recovered instruction')
  await expect(page.getByLabel('指令', { exact: true })).toHaveAttribute('aria-invalid', 'false')
  await page.getByRole('button', { name: '下一处问题', exact: true }).click()
  await expect(page.getByLabel('执行超时', { exact: true })).toBeFocused()
  await page.getByLabel('执行超时', { exact: true }).fill('2h')
  await expect(page.getByRole('button', { name: '下一处问题', exact: true })).toHaveCount(0)
  expect(requests.filter((r) => r.method === 'PUT')).toEqual([])
})

test('late node queries stay scoped and Save locks the whole draft until validation finishes', async ({
  page,
}) => {
  const f = await fixture(page)
  f.config.dag.nodes[1].parameters.model = 'model-b'
  f.config.dag.nodes[1].cwd = '/workspace/report'
  let releaseCatalog: (() => Promise<void>) | undefined
  let modelRequests = 0
  await page.route('**/models/query', async (route) => {
    if (++modelRequests === 1) {
      await new Promise<void>((resolve) => {
        releaseCatalog = async () => {
          await route.fulfill({
            json: {
              version: 1,
              models: [{ id: 'a', name: 'Late Model A', parameters: { model: 'model-a' } }],
            },
          })
          resolve()
        }
      })
    } else
      await route.fulfill({
        json: {
          version: 1,
          models: [{ id: 'b', name: 'Current Model B', parameters: { model: 'model-b' } }],
        },
      })
  })
  await page.goto(base + '?task=ux-fixture')
  await editNode(page)
  await expect.poll(() => modelRequests).toBe(1)
  await switchNode(page, 'Write report')
  await expect(page.getByRole('combobox', { name: '模型', exact: true })).toContainText(
    'Current Model B',
  )
  await releaseCatalog!()
  await expect(page.getByRole('combobox', { name: '模型', exact: true })).toContainText(
    'Current Model B',
  )
  await expect(page.getByLabel('指令', { exact: true })).toHaveValue('{{ nodes.source.result }}')
  let finishValidation: (() => Promise<void>) | undefined
  await page.route('**/tasks/validate', async (route) => {
    await new Promise<void>((resolve) => {
      finishValidation = async () => {
        await route.fulfill({ json: { errors: [] } })
        resolve()
      }
    })
  })
  await page.getByRole('tab', { name: '时间', exact: true }).click()
  await page.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(page.getByRole('tab', { name: '任务', exact: true })).toBeDisabled()
  await expect(page.getByRole('combobox', { name: '类型', exact: true })).toBeDisabled()
  await expect(page.getByRole('button', { name: '保存任务', exact: true })).toBeDisabled()
  await expect(page.getByRole('option')).toHaveCount(0)
  expect(f.requests.filter((r) => r.method === 'PUT')).toHaveLength(0)
  await expect.poll(() => !!finishValidation).toBe(true)
  await finishValidation!()
  await expect.poll(() => f.requests.filter((r) => r.method === 'PUT').length).toBe(1)
})

test('DAG name lives once in the top breadcrumb and remains editable on either step', async ({
  page,
}) => {
  await fixture(page)
  await page.goto(base + '?task=ux-fixture')
  const name = page.getByLabel('任务名称', { exact: true })
  await expect(
    page.locator('.header-breadcrumbs').getByLabel('任务名称', { exact: true }),
  ).toBeVisible()
  await expect(page.locator('.dag-stepbar input')).toHaveCount(0)
  await name.fill('')
  await name.pressSequentially('Research graph', { delay: 10 })
  await expect(name).toHaveValue('Research graph')
  await expect(name).toBeFocused()
  await page.getByRole('tab', { name: '时间', exact: true }).click()
  await expect(name).toBeVisible()
  await name.fill('')
  await page.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(name).toHaveAttribute('aria-invalid', 'true')
  await expect(name).toBeFocused()
  await name.fill('Daily report')
  await expect(name).toHaveAttribute('aria-invalid', 'false')
})
