import { test, expect, type Page } from '@playwright/test'
import { selectChoice } from './choices'
const workspace = '11111111-1111-4111-8111-111111111111'
const runnerId = '22222222-2222-4222-8222-222222222222'
const base = '/w/' + workspace + '/tasks/dag'
function definition() {
  const node = {
    id: 'source',
    name: 'Collect sources',
    pre: [] as string[],
    runner: runnerId,
    agent: 'pi',
    cwd: '/workspace',
    prompt: 'Find primary sources',
    parameters: { model: 'model-a' },
    session: null as { id: string } | null,
    timeout: '1h',
  }
  return {
    version: 1,
    id: 'ux-fixture',
    name: 'Daily report',
    enabled: false,
    schedule: { cron: '0 9 * * *', timezone: 'Asia/Shanghai' } as any,
    execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
    dag: {
      nodes: [
        node,
        {
          ...structuredClone(node),
          id: 'report',
          name: 'Write report',
          pre: ['source'],
          prompt: '{{ nodes.source.result }}',
        },
      ],
    },
  }
}
async function fixture(page: Page, config = definition()) {
  const requests: { path: string; method: string; body: any }[] = []
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname.replace(
      /^\/api\/v1(?:\/workspaces\/[0-9a-f-]{36})?/,
      '',
    )
    const method = route.request().method(),
      body = route.request().postDataJSON()
    requests.push({ path, method, body })
    let json: any = []
    if (path === '/session') json = { id: 'preview-user', username: 'preview', role: 'admin' }
    else if (path === '/meta') json = { version: '0.2.0' }
    else if (path === '/workspaces')
      json = { items: [{ id: workspace, name: 'Preview', role: 'admin' }] }
    else if (path === '/' + workspace) json = { id: workspace, name: 'Preview', role: 'admin' }
    else if (new URL(route.request().url()).pathname === '/api/v1/workspaces/' + workspace)
      json = { id: workspace, name: 'Preview', role: 'admin' }
    else if (path === '/settings') json = { timezone: 'Asia/Shanghai' }
    else if (path === '/runners')
      json = [
        {
          id: runnerId,
          name: 'Preview device',
          online: true,
          revoked: false,
          directory_roots: ['/workspace'],
          agents: [
            {
              id: 'pi',
              name: 'Pi',
              logo: '/agent-logos/pi.svg',
              available: true,
              mode: 'sync',
              digest: 'fixture',
              session_supported: true,
              session_list_supported: true,
              models_supported: true,
              parameters: {
                model: { type: 'string', label: 'Model' },
                reasoning: { type: 'string', label: 'Reasoning', enum: ['low', 'high'] },
              },
            },
          ],
        },
      ]
    else if (path === '/tasks/ux-fixture')
      json =
        method === 'PUT'
          ? { id: config.id, revision: 8, config: body.config }
          : { id: config.id, revision: 7, config, archived_at: null }
    else if (path.endsWith('/models/query'))
      json = {
        version: 1,
        models: [
          { id: 'a', name: 'Model A', parameters: { model: 'model-a' }, parameter_options: {} },
          { id: 'b', name: 'Model B', parameters: { model: 'model-b' }, parameter_options: {} },
        ],
      }
    else if (path.endsWith('/sessions/query'))
      json = { version: 1, sessions: [], next_cursor: null }
    else if (path.endsWith('/sessions/inspect'))
      json = { id: body.id, title: 'Preview session', cwd: '/workspace', persisted: true }
    else if (path === '/timezones') json = ['Asia/Shanghai', 'UTC', 'America/New_York']
    else if (path === '/schedule/preview') json = ['2026-10-20T01:00:00Z']
    else if (path === '/tasks/validate') json = { errors: [] }
    return route.fulfill({ json })
  })
  return { config, requests }
}
async function editNode(page: Page, id = 'source') {
  await page
    .locator('.dag-graph-node')
    .filter({ hasText: id === 'source' ? 'Collect sources' : 'Write report' })
    .click()
}
async function switchNode(page: Page, label: string) {
  await page
    .locator('.dag-node-context')
    .getByRole('combobox', { name: '选择节点', exact: true })
    .click()
  await page.getByRole('option', { name: new RegExp('^' + label) }).click()
}
test('graph, full-width editor and table use the shared card geometry without Runner UUIDs', async ({
  page,
}) => {
  await fixture(page)
  await page.goto(base + '?task=ux-fixture')
  await expect(page.locator('.dag-config-body')).toHaveCount(0)
  await expect(page.locator('.dag-graph-node').first()).toContainText('Preview device')
  await expect(page.locator('.dag-graph-node').first()).not.toContainText(runnerId)
  await editNode(page)
  const card = (await page.locator('.dag-nodes-card').boundingBox())!,
    form = (await page.locator('.dag-config-body').boundingBox())!
  expect(form.width).toBeGreaterThan(card.width - 4)
  await page.getByRole('button', { name: '表格', exact: true }).click()
  await expect(page.locator('.dag-node-table .dag-expanded-node')).toHaveCount(1)
  await expect(page.locator('.dag-node-table .dag-table-head')).toHaveCount(1)
  const row = page.locator('.dag-list-node').first(),
    head = page.locator('.dag-table-head')
  const deltas = await row.evaluate((el) =>
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
  expect(await head.evaluate((el) => getComputedStyle(el).fontSize)).toBe('11px')
  await expect(page.locator('.dag-graph-node')).toHaveCount(0)
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
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: 'test-results/dag-ux-editor-' + width + '-' + language + '-' + theme + '.png',
      fullPage: true,
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
    .locator('.dag-node-context')
    .getByRole('button', { name: '节点操作', exact: true })
    .click()
  await page.getByRole('menuitem', { name: '复制节点', exact: true }).click()
  await expect(page.getByLabel('名称', { exact: true })).toHaveValue(/Collect sources/)
  await expect(page.getByLabel('名称', { exact: true })).toBeFocused()
  await page
    .locator('.dag-node-context')
    .getByRole('button', { name: '节点操作', exact: true })
    .click()
  await page.getByRole('menuitem', { name: '新增后续节点', exact: true }).click()
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
  const transform = await page.locator('.dag-graph-plane').evaluate((el) => el.style.transform)
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
  const pane = page.locator('.dag-editor-scroll')
  await pane.evaluate((el) => {
    el.scrollTop = 300
  })
  await expect.poll(() => pane.evaluate((el) => el.scrollTop)).toBe(300)
  await page.getByRole('tab', { name: '时间', exact: true }).click()
  await page.getByRole('tab', { name: '任务', exact: true }).click()
  await expect.poll(() => pane.evaluate((el) => el.scrollTop)).toBe(300)
  expect(await page.locator('.dag-graph-plane').evaluate((el) => el.style.transform)).toBe(
    transform,
  )
  await expect(page.getByLabel('模型', { exact: true })).toHaveValue('private-model')
})
test('predecessor keyboard selection skips cycles and deletion points to the actual reference', async ({
  page,
}) => {
  await fixture(page)
  await page.goto(base + '?task=ux-fixture')
  await editNode(page)
  await page.getByRole('combobox', { name: '前置节点', exact: true }).click()
  await expect(page.getByRole('option', { name: /^Write report/ })).toBeDisabled()
  await page.keyboard.press('Escape')
  await page
    .locator('.dag-node-context')
    .getByRole('button', { name: '节点操作', exact: true })
    .click()
  await page.getByRole('menuitem', { name: '删除节点', exact: true }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('button', { name: '删除节点', exact: true })).toBeDisabled()
  await expect(dialog.getByRole('table')).toContainText('前置节点')
  await dialog.getByRole('button', { name: '前往处理', exact: true }).first().click()
  await expect(page.getByRole('combobox', { name: '前置节点', exact: true })).toBeFocused()
  await expect(page.getByLabel('名称', { exact: true })).toHaveValue('Write report')
  await page.getByRole('combobox', { name: '前置节点', exact: true }).click()
  const input = page.getByRole('combobox', { name: '搜索前置节点', exact: true })
  await input.fill('source')
  await input.press('ArrowDown')
  await input.press('Enter')
  await expect(page.getByRole('option', { name: /^Collect sources/ })).toHaveAttribute(
    'aria-selected',
    'false',
  )
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
