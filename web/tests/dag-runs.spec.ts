import { test, expect, type Page } from '@playwright/test'
const workspace = '11111111-1111-4111-8111-111111111111'
const id = '00000000-0000-4000-8000-000000000001'
const child = '00000000-0000-4000-8000-000000000002'
const at = '2026-10-20T10:00:00Z'
function fixture() {
  const node = {
    id: 'collect',
    name: 'Collect sources',
    pre: [],
    runner: 'Preview device',
    agent: 'pi',
    cwd: '/workspace',
    prompt: 'Collect sources',
    parameters: {},
    timeout: '1h',
  }
  const config = {
    version: 1,
    id: 'preview-dag',
    name: 'Daily research',
    runner: '',
    agent: '',
    cwd: '',
    prompt: '',
    parameters: {},
    schedule: null,
    enabled: true,
    execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
    dag: { nodes: [node] },
  }
  const run: any = {
    id,
    task_id: config.id,
    state: 'running',
    reason: null,
    result: null,
    task_archived: false,
    config,
    source: 'manual',
    created_at: at,
    scheduled_at: at,
    started_at: at,
    finished_at: null,
    nodes: [{ ...node, state: 'running', execution_id: child, continuation_available: true }],
  }
  const execution = {
    ...run,
    id: child,
    parent_run_id: id,
    node_id: node.id,
    nodes: undefined,
    config: { ...config, ...node, dag: undefined },
    execution_snapshot: { trace_supported: true, cwd: '/workspace', parameters: {} },
    session: {
      id: 'native-session',
      runner_id: 'preview-device',
      agent: 'pi',
      cwd: '/workspace',
      ready: true,
      command: 'cd -- /workspace && pi --resume native-session',
    },
  }
  const events = [
    {
      id: 1,
      run_id: child,
      seq: 1,
      kind: 'trace',
      created_at: at,
      data: {
        id: 'tool',
        kind: 'tool',
        status: 'succeeded',
        title: 'Read sources',
        input: { file: 'sources.md' },
        output: 'Primary sources',
        occurred_at: at,
        started_at: at,
        finished_at: '2026-10-20T10:00:01Z',
      },
    },
  ]
  return { run, execution, events }
}
async function mock(page: Page, f: ReturnType<typeof fixture>, role = 'admin') {
  const writes: string[] = []
  await page.route('**/api/v1/**', async (route) => {
    const url = new URL(route.request().url()),
      path = url.pathname
    if (route.request().method() !== 'GET') writes.push(path)
    let value: any = []
    if (path === '/api/v1/session') value = { id: 'preview-user', username: 'preview', role }
    else if (path === '/api/v1/meta') value = { version: '0.2.0' }
    else if (path === `/api/v1/workspaces/${workspace}`)
      value = { id: workspace, name: 'Preview', role }
    else if (path.endsWith('/workspaces'))
      value = { items: [{ id: workspace, name: 'Preview', role }], demo: null }
    else if (path.endsWith('/settings')) value = { timezone: 'UTC' }
    else if (path.endsWith('/runs')) value = { items: [f.run], total: 1, page: 1, page_size: 20 }
    else if (path.endsWith(`/runs/${id}`)) value = f.run
    else if (path.endsWith('/nodes/collect'))
      value = { node: f.run.nodes[0], execution: f.execution }
    else if (path.endsWith('/events'))
      value = f.events.filter((e) => e.id > Number(url.searchParams.get('after') ?? 0))
    else if (path.endsWith('/stream'))
      return route.fulfill({ contentType: 'text/event-stream', body: ': fixture\n\n' })
    else if (path.endsWith('/agents'))
      value = [
        {
          id: 'pi',
          name: 'Pi',
          runner: 'Preview device',
          runner_id: 'preview-device',
          logo: '/agent-logos/pi.svg',
          available: true,
        },
      ]
    else if (path.endsWith('/runners'))
      value = [{ id: 'preview-device', name: 'Preview device', agents: [] }]
    else if (path.endsWith('/moments/days'))
      value = {
        timezone: 'UTC',
        today: '2026-10-20',
        total_runs: 1,
        days: [{ date: '2026-10-20', count: 1 }],
        next_before: null,
      }
    else if (path.endsWith('/moments'))
      value = {
        timezone: 'UTC',
        date: '2026-10-20',
        items: [
          {
            id,
            task_id: 'preview-dag',
            name: 'Daily research',
            state: 'warning',
            reason: 'Independent branch failed',
            finished_at: at,
            format: 'dag',
            node_count: 2,
            continuation_available: true,
            preview: null,
            outputs: [
              {
                node_id: 'collect',
                name: 'Collect sources',
                state: 'succeeded',
                preview: '# Preview report',
                format: 'markdown',
                has_more: true,
                truncated: false,
              },
            ],
            has_more: true,
            truncated: false,
          },
        ],
        next_cursor: null,
      }
    return route.fulfill({ json: value })
  })
  return writes
}
function finish(f: ReturnType<typeof fixture>) {
  f.run.state = 'warning'
  f.run.reason = 'Independent branch failed'
  f.run.finished_at = at
  f.run.result = {
    type: 'dag',
    outputs: [
      {
        node_id: 'collect',
        name: 'Collect sources',
        state: 'succeeded',
        result: '# Final report\n\n| Source | Status |\n|---|---|\n| Primary | Ready |',
      },
    ],
  }
  f.run.nodes[0].state = 'succeeded'
  f.execution.state = 'succeeded'
}
test('DAG highlights output once, preserves trace expansion and uses only whole-graph actions', async ({
  page,
}) => {
  const f = fixture(),
    writes = await mock(page, f)
  await page.goto(`/w/${workspace}/runs?run=${id}`)
  await expect(page.locator('.dag-graph-node')).toHaveCount(1)
  await page.getByText('Read sources', { exact: true }).click()
  await expect(page.getByText('Primary sources', { exact: true })).toBeVisible()
  finish(f)
  await expect(page.getByRole('heading', { name: 'Final report', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '过程', exact: true })).toHaveAttribute(
    'aria-expanded',
    'false',
  )
  await page.getByRole('button', { name: '过程', exact: true }).click()
  await expect(page.getByText('Primary sources', { exact: true })).toBeVisible()
  await page.waitForTimeout(2300)
  await expect(page.getByRole('button', { name: '过程', exact: true })).toHaveAttribute(
    'aria-expanded',
    'true',
  )
  await page.getByRole('button', { name: '原文', exact: true }).click()
  await expect(page.locator('.result-raw')).toContainText('"type": "dag"')
  await page.getByRole('button', { name: '阅读', exact: true }).click()
  await expect(page.locator('.trace-notice')).toHaveCount(0)
  await page.locator('.dag-run-page').evaluate((el) => (el.scrollTop = 0))
  for (const [width, theme] of [
    [1440, 'light'],
    [390, 'dark'],
    [2200, 'light'],
  ] as const) {
    await page.setViewportSize({ width, height: 900 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({ path: `test-results/dag-run-${width}-${theme}.png`, fullPage: true })
  }
  expect(writes).toEqual([])
})
test('DAG completion does not leave details; Moments previews reports and resumes the selected actual node', async ({
  page,
}) => {
  const f = fixture()
  await mock(page, f)
  await page.goto(`/w/${workspace}/runs?run=${id}`)
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  finish(f)
  await expect(page.getByRole('tab', { name: '详情', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await page.getByRole('tab', { name: '执行', exact: true }).click()
  await expect(page.getByRole('heading', { name: 'Final report', exact: true })).toBeVisible()
  await page.goto(`/w/${workspace}/`)
  await expect(page.getByRole('heading', { name: 'Preview report', exact: true })).toBeVisible()
  await expect(page.locator('.moment-identity')).toContainText('2')
  await expect(page.locator('.moment-identity')).not.toContainText('Preview device')
  await page.getByRole('button', { name: '阅读全文', exact: true }).click()
  await expect(page.getByRole('heading', { name: 'Final report', exact: true })).toBeVisible()
  await page.getByRole('button', { name: '续聊', exact: true }).click()
  await expect(page.getByRole('dialog')).toContainText('pi --resume native-session')
  await expect(page.getByRole('dialog')).toContainText('/workspace')
})

test('DAG node details has one card header, embedded folds and preserved follow/selection state', async ({
  page,
}) => {
  const f = fixture()
  await mock(page, f)
  await page.goto(`/w/${workspace}/runs?run=${id}`)
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  const details = page.locator('.dag-node-details:visible')
  await expect(details).toHaveCount(1)
  await expect(details.locator('.content-card')).toHaveCount(0)
  await expect(details.getByRole('combobox', { name: '选择节点', exact: true })).toHaveCount(0)
  await expect(details.locator('.dag-node-context strong')).toHaveText('Collect sources')
  await expect(details.getByRole('button', { name: '日志', exact: true })).toHaveAttribute(
    'aria-expanded',
    'true',
  )
  await expect(details.getByRole('button', { name: '执行信息', exact: true })).toHaveAttribute(
    'aria-expanded',
    'false',
  )
  await details.getByLabel('跟随输出', { exact: true }).uncheck()
  await details.getByRole('button', { name: '执行信息', exact: true }).click()
  await page.getByRole('tab', { name: '执行', exact: true }).click()
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await expect(details.getByLabel('跟随输出', { exact: true })).not.toBeChecked()
  await expect(details.getByRole('button', { name: '执行信息', exact: true })).toHaveAttribute(
    'aria-expanded',
    'true',
  )
  for (const [width, language, theme] of [
    [1440, 'zh', 'light'],
    [390, 'zh', 'dark'],
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
      path: `test-results/dag-details-${width}-${language}-${theme}.png`,
      fullPage: true,
    })
  }
})
test('DAG multi-node details switches context without duplicate titles and keeps reconciliation on the actual node', async ({
  page,
}) => {
  const f = fixture()
  f.run.state = 'unknown'
  f.run.nodes.push({
    ...f.run.nodes[0],
    id: 'uncertain',
    name: 'Uncertain branch',
    state: 'unknown',
    execution_id: '00000000-0000-4000-8000-000000000003',
    continuation_available: false,
  })
  await mock(page, f)
  await page.route('**/nodes/uncertain', (route) =>
    route.fulfill({
      json: {
        node: f.run.nodes[1],
        execution: {
          ...f.execution,
          id: f.run.nodes[1].execution_id,
          state: 'unknown',
          session: null,
        },
      },
    }),
  )
  await page.goto(`/w/${workspace}/runs?run=${id}`)
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  const details = page.locator('.dag-node-details:visible')
  await details.getByRole('combobox', { name: '选择节点', exact: true }).click()
  await page.getByRole('option', { name: /^Uncertain branch/ }).click()
  await expect(details).toHaveCount(1)
  await expect(details.locator('.dag-node-context .status')).toHaveText('状态待核对')
  await details.getByRole('button', { name: '核对状态', exact: true }).click()
  await expect(page.getByRole('dialog')).toBeVisible()
  await page.getByRole('dialog').getByRole('button', { name: '取消', exact: true }).click()
  await page.getByRole('tab', { name: '执行', exact: true }).click()
  await expect(
    page
      .locator('.dag-node-process:visible')
      .getByRole('combobox', { name: '选择节点', exact: true }),
  ).toContainText('Uncertain branch')
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await expect(details.getByRole('combobox', { name: '选择节点', exact: true })).toContainText(
    'Uncertain branch',
  )
})
test('DAG skipped nodes retain their real reason and Viewer cannot reconcile, retry or cancel', async ({
  page,
}) => {
  const f = fixture()
  f.run.state = 'failed'
  f.run.reason = 'Required output was skipped'
  f.run.finished_at = at
  f.run.nodes[0].state = 'skipped'
  f.run.nodes[0].execution_id = null
  f.run.nodes[0].reason = 'Predecessor did not succeed'
  await mock(page, f, 'viewer')
  await page.goto(`/w/${workspace}/runs?run=${id}`)
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await expect(page.locator('.dag-node-details:visible')).toContainText(
    'Predecessor did not succeed',
  )
  await expect(page.getByRole('button', { name: '核对状态', exact: true })).toHaveCount(0)
  await expect(page.getByRole('button', { name: '重试', exact: true })).toHaveCount(0)
  await expect(page.getByRole('button', { name: '取消执行', exact: true })).toHaveCount(0)
})

test('an empty successful result is explicit and its fold choice survives tab changes', async ({
  page,
}) => {
  const f = fixture()
  f.run.state = 'succeeded'
  f.run.finished_at = at
  f.run.nodes[0].state = 'succeeded'
  await mock(page, f)
  await page.goto(`/w/${workspace}/runs?run=${id}`)
  const result = page
    .locator('.run-section')
    .filter({ has: page.getByRole('button', { name: '结果', exact: true }) })
  await expect(result).toContainText('未返回结果')
  await result.getByRole('button', { name: '结果', exact: true }).click()
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await page.getByRole('tab', { name: '执行', exact: true }).click()
  await expect(result.getByRole('button', { name: '结果', exact: true })).toHaveAttribute(
    'aria-expanded',
    'false',
  )
})

test('Run graph shares the overview canvas controls and fullscreen is read-only', async ({
  page,
}) => {
  const f = fixture(),
    writes = await mock(page, f, 'viewer')
  await page.goto(`/w/${workspace}/runs?run=${id}`)
  const graph = page.locator('.dag-structure'),
    controls = graph.getByRole('toolbar', { name: '画布操作', exact: true })
  await expect(graph.locator('.dag-graph-node')).toHaveCount(1)
  await expect(controls.getByRole('button')).toHaveCount(3)
  await expect(graph.getByRole('button', { name: '适应视图', exact: true })).toHaveCount(0)
  await controls.getByRole('button', { name: '全屏', exact: true }).click()
  await expect(graph.locator('.dag-structure-fullscreen')).toBeVisible()
  await page.mouse.move(600, 400)
  await page.keyboard.press('Escape')
  await expect(graph.locator('.dag-structure-fullscreen')).toHaveCount(0)
  expect(writes).toEqual([])
})

test('retry acceleration targets the reserved execution and historical attempts retain their own result and session', async ({
  page,
}) => {
  const f = fixture(),
    old = '00000000-0000-4000-8000-000000000003'
  f.run.nodes[0] = {
    ...f.run.nodes[0],
    state: 'queued',
    reason: 'Current attempt reason',
    started_at: '2026-10-20T10:00:00Z',
    finished_at: '2026-10-20T10:01:00Z',
    attempt_count: 2,
    retry_at: '2099-01-01T00:00:00Z',
  }
  f.execution.state = 'queued'
  await mock(page, f)
  const writes: any[] = []
  const selected: string[] = []
  await page.route('**/nodes/collect?**', (route) => {
    const execution = new URL(route.request().url()).searchParams.get('execution')!
    selected.push(execution)
    return route.fulfill({
      json: {
        node: f.run.nodes[0],
        attempts: [
          { id: old, state: 'failed' },
          { id: child, state: 'queued' },
        ],
        execution:
          execution === old
            ? {
                ...f.execution,
                id: old,
                state: 'failed',
                reason: 'First attempt reason',
                started_at: '2026-10-20T10:00:00Z',
                finished_at: '2026-10-20T10:00:07Z',
                result: '# First attempt',
                session: {
                  ...f.execution.session,
                  id: 'old-session',
                  command: 'cd -- /workspace && pi --resume old-session',
                },
              }
            : f.execution,
      },
    })
  })
  await page.route('**/nodes/collect/retry', (route) => {
    writes.push(route.request().postDataJSON())
    f.run.nodes[0].retry_at = null
    return route.fulfill({ json: { id: child } })
  })
  await page.goto(`/w/${workspace}/runs?run=${id}`)
  await page.getByRole('button', { name: '立即重试', exact: true }).click()
  expect(writes).toEqual([{ execution_id: child }])
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  const details = page.locator('.dag-node-details:visible')
  await details.getByRole('combobox', { name: '执行尝试', exact: true }).click()
  await page.getByRole('option').filter({ hasText: '第 1 次' }).click()
  await expect(details.getByRole('heading', { name: 'First attempt', exact: true })).toBeVisible()
  await expect(details.locator('.run-reason')).toContainText('First attempt reason')
  await expect(details.locator('.dag-node-context')).toContainText('7.0s')
  await expect(details).not.toContainText('Current attempt reason')
  expect(selected).toContain(old)
  await details.getByRole('button', { name: '续聊', exact: true }).click()
  await expect(page.getByRole('dialog')).toContainText('pi --resume old-session')
})

for (const [width, language, theme] of [
  [1440, 'en', 'light'],
  [1440, 'zh', 'dark'],
  [390, 'en', 'dark'],
  [390, 'zh', 'light'],
] as const) {
  test(`DAG node menu keeps aligned columns and compact timing at ${width}px in ${language}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 })
    const f = fixture()
    const states = ['succeeded', 'running', 'unknown', 'failed', 'skipped']
    f.run.nodes = Array.from({ length: 100 }, (_, index) => ({
      ...f.run.nodes[0],
      id: index === 0 ? 'collect' : `research-node-${String(index).padStart(3, '0')}-full-id`,
      name: `Synthetic research ${index + 1} — 多语言节点名称与较长的研究主题`,
      state: states[index % states.length],
      started_at: at,
      finished_at: '2026-10-20T10:06:41.800Z',
    }))
    const writes = await mock(page, f)
    await page.route('**/nodes/*', async (route) => {
      const node = f.run.nodes.find((n: any) =>
        new URL(route.request().url()).pathname.endsWith(`/nodes/${n.id}`),
      )
      return route.fulfill({
        json: {
          node,
          execution: {
            ...f.execution,
            node_id: node.id,
            state: node.state,
            started_at: node.started_at,
            finished_at: node.finished_at,
          },
        },
      })
    })
    await page.goto(`/w/${workspace}/runs?run=${id}`)
    await page.evaluate(
      async ({ language, theme }) => {
        const module = await import('/src/i18n.ts')
        await module.setLanguage(language)
        document.documentElement.dataset.theme = theme
      },
      { language, theme },
    )
    await page
      .getByRole('tab', { name: language === 'en' ? 'Details' : '详情', exact: true })
      .click()
    const details = page.locator('.dag-node-details:visible')
    const trigger = details.locator('.dag-node-context > .choice-selector')
    await expect(trigger).toHaveAttribute('title', f.run.nodes[0].name)
    await expect(details.locator('.dag-node-duration')).toHaveText('401.8s')
    expect(await trigger.evaluate((el) => getComputedStyle(el).fontSize)).toBe('12px')
    for (const selector of ['.dag-node-context > .status', '.dag-node-duration']) {
      expect(await details.locator(selector).evaluate((el) => getComputedStyle(el).fontSize)).toBe(
        '11px',
      )
    }
    await trigger.click()
    const menu = page.locator('.dag-node-picker')
    const search = menu.getByRole('combobox')
    await expect(search).toHaveAttribute(
      'placeholder',
      language === 'en' ? 'Search nodes…' : '搜索节点…',
    )
    await expect(menu.getByRole('option')).toHaveCount(100)
    const geometry = await menu.evaluate((el) => {
      const list = el.querySelector('.model-options')!
      const rows = [...list.querySelectorAll('button')]
      return {
        width: el.getBoundingClientRect().width,
        left: el.getBoundingClientRect().left,
        right: el.getBoundingClientRect().right,
        fits: [el, list, ...rows].every((item) => item.scrollWidth <= item.clientWidth),
        height: Math.max(...rows.map((row) => row.getBoundingClientRect().height)),
        statusX: rows.map((row) => row.querySelector('.status')!.getBoundingClientRect().x),
        labelWidths: rows.map(
          (row) => row.querySelector('.choice-option-label')!.getBoundingClientRect().width,
        ),
        checkX: rows[0].querySelector('.choice-option-meta > svg')!.getBoundingClientRect().x,
      }
    })
    expect(geometry.width).toBe(width === 390 ? 366 : 420)
    expect(geometry.left).toBeGreaterThanOrEqual(12)
    expect(geometry.right).toBeLessThanOrEqual(width - 12)
    expect(geometry.fits).toBe(true)
    expect(geometry.height).toBeLessThanOrEqual(54)
    expect(Math.max(...geometry.statusX) - Math.min(...geometry.statusX)).toBeLessThan(1)
    expect(Math.max(...geometry.labelWidths) - Math.min(...geometry.labelWidths)).toBeLessThan(1)
    await expect(menu.locator('.choice-option-label').first()).toHaveAttribute(
      'title',
      f.run.nodes[0].name,
    )
    await page.screenshot({ path: `test-results/dag-node-menu-${width}-${language}-${theme}.png` })
    await search.press('End')
    await expect(menu.getByRole('option').last()).toHaveAttribute('data-active', '')
    await expect(menu.getByRole('option').last()).toBeInViewport()
    await search.press('Home')
    await search.press('ArrowDown')
    await search.press('Enter')
    await expect(trigger).toContainText(f.run.nodes[1].name)
    await expect(trigger).toBeFocused()
    await trigger.press('ArrowDown')
    const checkX = await menu
      .locator('.choice-option-meta > svg')
      .evaluate((el) => el.getBoundingClientRect().x)
    expect(Math.abs(checkX - geometry.checkX)).toBeLessThan(1)
    await search.fill(f.run.nodes[99].id)
    await expect(menu.getByRole('option')).toHaveCount(1)
    await search.press('Enter')
    await expect(trigger).toContainText(f.run.nodes[99].name)
    await trigger.click()
    await search.fill('no matching fixture node')
    await expect(menu.getByRole('option')).toHaveCount(0)
    await search.press('Escape')
    await expect(menu).toHaveCount(0)
    await expect(trigger).toBeFocused()
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    expect(writes).toEqual([])
  })
}
