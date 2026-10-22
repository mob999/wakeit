import { test, expect, type Page } from '@playwright/test'
import { readFileSync } from 'node:fs'
const zh = JSON.parse(readFileSync(new URL('../src/locales/zh.json', import.meta.url), 'utf8'))
const en = JSON.parse(readFileSync(new URL('../src/locales/en.json', import.meta.url), 'utf8'))
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
      value = {
        node: f.run.nodes[0],
        execution: f.execution,
        retry: (f as any).retry,
        attempts: (f as any).attempts,
      }
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
  await expect(page.locator('.result-raw')).toContainText('# Final report')
  await expect(page.locator('.result-raw')).not.toContainText('"type": "dag"')
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
  await page.getByRole('link', { name: '查看运行', exact: true }).click()
  await expect(page.getByRole('heading', { name: 'Final report', exact: true })).toBeVisible()
  await page.getByRole('tab', { name: '详情', exact: true }).click()
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
  const f = fixture()
  ;(f as any).retry = { action: 'accelerate', affected: [] }
  const old = '00000000-0000-4000-8000-000000000003'
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
        retry: (f as any).retry,
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
  expect(writes).toEqual([{ execution_id: child, resume_stopped: false }])
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  const details = page.locator('.dag-node-details:visible')
  await details.getByRole('combobox', { name: '执行尝试', exact: true }).click()
  await page.getByRole('option').filter({ hasText: '第 1 次' }).click()
  await expect(details.getByRole('heading', { name: 'First attempt', exact: true })).toBeVisible()
  await expect(details.locator('.run-reason')).toHaveAttribute(
    'aria-label',
    '失败原因: First attempt reason',
  )
  await expect(details.locator('.dag-node-context')).toContainText('7.0s')
  await expect(details).not.toContainText('Current attempt reason')
  expect(selected).toContain(old)
  await details.getByRole('button', { name: '续聊', exact: true }).click()
  await expect(page.getByRole('dialog')).toContainText('pi --resume old-session')
})

test('issue navigation restores the current attempt without resetting reading or structure state', async ({
  page,
}) => {
  await page.addInitScript(() => {
    window.EventSource = class extends EventTarget {
      close() {}
    } as unknown as typeof EventSource
  })
  const f = fixture(),
    old = '00000000-0000-4000-8000-000000000003'
  f.run.state = 'failed'
  f.run.reason = null
  f.run.nodes[0].state = 'failed'
  f.execution.state = 'failed'
  f.execution.reason = 'Current attempt failure'
  const writes = await mock(page, f, 'viewer')
  await page.route('**/nodes/collect?**', (route) => {
    const execution = new URL(route.request().url()).searchParams.get('execution')
    return route.fulfill({
      json: {
        node: f.run.nodes[0],
        attempts: [
          { id: old, state: 'failed' },
          { id: child, state: 'failed' },
        ],
        execution:
          execution === old
            ? { ...f.execution, id: old, reason: 'Old attempt failure', result: '# Old attempt' }
            : f.execution,
      },
    })
  })
  await page.goto(`/w/${workspace}/runs?run=${id}`)
  await page.getByRole('button', { name: '收起结构', exact: true }).click()
  await page.getByText('Read sources', { exact: true }).click()
  await expect(page.getByText('Primary sources', { exact: true })).toBeVisible()
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  const details = page.locator('.dag-node-details:visible')
  await details.getByRole('combobox', { name: '执行尝试', exact: true }).click()
  await page.getByRole('option').filter({ hasText: '第 1 次' }).click()
  await expect(details.locator('.run-reason')).toHaveAttribute(
    'aria-label',
    '失败原因: Old attempt failure',
  )
  await page.getByRole('button', { name: '异常节点 1', exact: true }).click()
  await page.getByRole('menuitem', { name: 'Collect sources 失败 collect', exact: true }).click()
  await expect(page.getByRole('tab', { name: '执行', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  const process = page.locator('.dag-node-process:visible')
  await expect(process.locator('.run-trace')).toHaveAttribute('data-execution', child)
  await expect(process.locator('.run-reason')).toHaveAttribute(
    'aria-label',
    '失败原因: Current attempt failure',
  )
  await expect(process.getByText('Primary sources', { exact: true })).toBeVisible()
  await expect(page.locator('.dag-run-structure')).toBeHidden()
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await expect(details.locator('.run-reason')).toHaveAttribute(
    'aria-label',
    '失败原因: Current attempt failure',
  )
  await expect(details.getByRole('combobox', { name: '执行尝试', exact: true })).toContainText(
    '第 2 次',
  )
  await expect(page.getByRole('button', { name: '重试', exact: true })).toHaveCount(0)
  expect(writes).toEqual([])
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
    const trigger = details.getByRole('combobox', {
      name: language === 'en' ? 'Select a node' : '选择节点',
      exact: true,
    })
    await expect(trigger).toHaveAttribute('title', f.run.nodes[0].name)
    await expect(details.locator('.dag-node-duration')).toHaveText('401.8s')
    expect(await trigger.evaluate((el) => getComputedStyle(el).fontSize)).toBe('12px')
    for (const selector of ['.dag-node-context .status', '.dag-node-duration']) {
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

test('manual retry uses the current guard, preserves selection and requires explicit stopped confirmation', async ({
  page,
}) => {
  const f = fixture()
  f.run.state = f.execution.state = f.run.nodes[0].state = 'failed'
  f.run.finished_at = f.execution.finished_at = at
  ;(f as any).retry = { action: 'retry', affected: [] }
  await mock(page, f)
  const requests: any[] = []
  await page.route('**/nodes/collect/retry', async (route) => {
    requests.push(route.request().postDataJSON())
    return route.fulfill({
      status: 409,
      json: { error: 'Execution changed; refresh before retrying' },
    })
  })
  await page.goto(`/w/${workspace}/runs?run=${id}`)
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 })
    for (const tab of ['执行', '详情']) {
      await page.getByRole('tab', { name: tab, exact: true }).click()
      const context = page.locator('.dag-node-context:visible')
      await expect(
        context
          .locator('.dag-node-context-actions')
          .getByRole('button', { name: '重试节点', exact: true }),
      ).toBeVisible()
      await expect(context.locator(':scope > .icon-button')).toHaveCount(0)
      const centers = await context.evaluate((el) => {
        const center = (element: Element) => {
          const rect = element.getBoundingClientRect()
          return rect.y + rect.height / 2
        }
        return [
          center(el.querySelector('[aria-label="重试节点"]')!),
          center(el.querySelector('.status')!),
          center(el.querySelector('.dag-node-duration')!),
        ]
      })
      expect(Math.max(...centers) - Math.min(...centers)).toBeLessThan(1)
    }
  }
  await page.getByRole('tab', { name: '执行', exact: true }).click()
  await page.getByRole('button', { name: '重试节点', exact: true }).first().click()
  await expect(page.getByRole('alert')).toContainText('当前执行已变更，请刷新后重试。')
  expect(requests[0]).toEqual({ execution_id: child, resume_stopped: false })
  f.run.state = f.execution.state = f.run.nodes[0].state = 'cancelled'
  ;(f as any).retry = {
    action: 'resume',
    affected: [{ id: 'after', name: 'Downstream report', state: 'cancelled' }],
  }
  await expect(page.getByRole('button', { name: '恢复节点', exact: true }).first()).toBeVisible()
  await page.getByRole('button', { name: '恢复节点', exact: true }).first().click()
  const dialog = page.getByRole('dialog')
  await expect(dialog).toContainText('保留成功节点')
  await expect(dialog.getByRole('table')).toContainText('Downstream report')
  expect(requests).toHaveLength(1)
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 })
    await expect(dialog).toBeVisible()
    await page.screenshot({ path: `../.local/review/recovery-dialog-${width}.png` })
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  }
  await dialog.getByRole('button', { name: '恢复节点', exact: true }).click()
  await expect.poll(() => requests.length).toBe(2)
  expect(requests[1]).toEqual({ execution_id: child, resume_stopped: true })
  await expect(dialog).toBeVisible() // conflict retains context instead of silently switching attempts
})

test('uncreated failed execution supports retry and historical attempts do not expose the action', async ({
  page,
}) => {
  const f = fixture()
  f.run.state = f.execution.state = f.run.nodes[0].state = 'failed'
  f.run.nodes[0].execution_id = null as any
  ;(f as any).retry = { action: 'retry', affected: [] }
  await mock(page, f)
  const bodies: any[] = []
  await page.route('**/nodes/collect/retry', async (route) => {
    bodies.push(route.request().postDataJSON())
    return route.fulfill({ json: { id: child, parent_run_id: id, node_id: 'collect' } })
  })
  await page.goto(`/w/${workspace}/runs?run=${id}`)
  await page.getByRole('button', { name: '重试节点', exact: true }).first().click()
  await expect.poll(() => bodies.length).toBe(1)
  expect(bodies[0].execution_id).toBeNull()
})

for (const language of ['zh', 'en'] as const) {
  test(`node retry controls, confirmation, blockers and Toast follow ${language} and live language changes`, async ({
    page,
  }) => {
    const messages = language === 'zh' ? zh : en
    const other = language === 'zh' ? en : zh
    await page.addInitScript(
      (language) => localStorage.setItem('wakeit-language', language),
      language,
    )
    const f = fixture()
    f.run.state = f.execution.state = f.run.nodes[0].state = 'failed'
    f.run.finished_at = f.execution.finished_at = at
    ;(f as any).retry = { action: 'retry', affected: [] }
    ;(f as any).attempts = [
      { id: child, state: 'failed' },
      { id: 'older', state: 'failed' },
    ]
    await mock(page, f)
    await page.route('**/nodes/collect/retry', (route) =>
      route.fulfill({ status: 409, json: { error: 'Execution changed; refresh before retrying' } }),
    )
    await page.goto(`/w/${workspace}/runs?run=${id}`)
    const retry = page.getByRole('button', { name: messages['retry.node'], exact: true }).first()
    await retry.hover()
    await expect(page.getByRole('tooltip')).toHaveText(messages['retry.node'])
    await expect(
      page.getByRole('combobox', { name: messages['retry.attempts'], exact: true }).first(),
    ).toBeVisible()
    for (const width of [1440, 390]) {
      await page.setViewportSize({ width, height: 900 })
      const identity = page.locator('.dag-node-context:visible .dag-node-identity')
      const centers = await identity.evaluate((el) =>
        Array.from(el.children).map((child) => {
          const box = child.getBoundingClientRect()
          return box.y + box.height / 2
        }),
      )
      expect(Math.max(...centers) - Math.min(...centers)).toBeLessThan(1)
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
        true,
      )
    }
    const attempt = page
      .getByRole('combobox', { name: messages['retry.attempts'], exact: true })
      .first()
    await attempt.click()
    const menu = page.locator('.dag-attempt-menu')
    await expect(menu).toBeVisible()
    const rows = await menu.getByRole('option').evaluateAll((elements) =>
      elements.map((el) => {
        const row = el.getBoundingClientRect()
        const label = el.querySelector('.choice-option-label')!.getBoundingClientRect()
        const status = el.querySelector('.choice-option-meta')!.getBoundingClientRect()
        return {
          height: row.height,
          labelHeight: label.height,
          statusX: status.x,
          width: row.width,
        }
      }),
    )
    expect(rows.every((row) => row.labelHeight <= 20 && row.height <= 40 && row.width >= 190)).toBe(
      true,
    )
    expect(new Set(rows.map((row) => row.statusX)).size).toBe(1)
    await page.screenshot({ path: `../.local/review/attempt-menu-${language}.png` })
    await menu.getByRole('listbox').press('Escape')
    await page.setViewportSize({ width: 1440, height: 900 })
    await retry.click()
    await expect(page.getByRole('alert')).toContainText(
      messages['Execution changed; refresh before retrying'],
    )
    ;(f as any).retry = { action: 'accelerate', affected: [] }
    f.run.nodes[0].retry_at = '2099-01-01T00:00:00Z'
    await expect(
      page.getByRole('button', { name: messages['retry.now'], exact: true }).first(),
    ).toBeVisible()
    await expect(page.locator('.dag-retry-time:visible')).toContainText(messages['retry.waiting'])
    ;(f as any).retry = {
      action: 'resume',
      affected: [{ id: 'after', name: 'Downstream report', state: 'cancelled' }],
    }
    await page.getByRole('button', { name: messages['retry.resume'], exact: true }).first().click()
    const dialog = page.getByRole('dialog')
    await expect(dialog).toContainText(messages['retry.preserve'])
    await expect(
      dialog.getByRole('columnheader').filter({ hasText: messages['操作'] }),
    ).toBeVisible()
    await expect(
      dialog.getByRole('cell', { name: messages['retry.continue'], exact: true }),
    ).toBeVisible()
    await page.evaluate(
      async (language) => {
        const module = await import('/src/i18n.ts')
        await module.setLanguage(language)
      },
      language === 'zh' ? 'en' : 'zh',
    )
    await expect(dialog.getByRole('heading')).toHaveText(other['retry.resume'])
    await expect(dialog).toContainText(other['retry.preserve'])
    await dialog.getByRole('button', { name: other['取消'], exact: true }).click()
    ;(f as any).retry = {
      action: null,
      reason: 'Node in state succeeded cannot be retried',
      affected: [],
    }
    const blocked = other['retry.error.state'].replace('{{state}}', other['成功'])
    await expect(page.getByRole('button', { name: blocked, exact: true }).first()).toBeDisabled()
    await page.evaluate(async (language) => {
      const module = await import('/src/i18n.ts')
      await module.setLanguage(language)
    }, language)
    await expect(
      page
        .getByRole('button', {
          name: messages['retry.error.state'].replace('{{state}}', messages['成功']),
          exact: true,
        })
        .first(),
    ).toBeDisabled()
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  })
}

test('recording tooltips belong to the selected DAG execution, never another node', async ({
  page,
}) => {
  const f = fixture()
  f.run.nodes.push({
    ...f.run.nodes[0],
    id: 'other',
    name: 'Other branch',
    execution_id: '00000000-0000-4000-8000-000000000003',
  })
  await mock(page, f)
  await page.route('**/nodes/other', (route) =>
    route.fulfill({
      json: {
        node: f.run.nodes[1],
        execution: { ...f.execution, id: f.run.nodes[1].execution_id, node_id: 'other' },
      },
    }),
  )
  await page.route('**/runs/*/events?*', (route) => {
    const url = new URL(route.request().url())
    return route.fulfill({
      json:
        url.pathname.includes(child) && !Number(url.searchParams.get('after'))
          ? [
              ...f.events,
              {
                id: 2,
                run_id: child,
                seq: 2,
                kind: 'trace_status',
                created_at: at,
                data: { status: 'truncated', reason: 'content_shortened' },
              },
            ]
          : [],
    })
  })
  await page.goto(`/w/${workspace}/runs?run=${id}`)
  const info = page.getByRole('button', { name: '过程记录提示', exact: true })
  await expect(info).toHaveCount(1)
  const picker = page.getByRole('combobox', { name: '选择节点', exact: true }).first()
  await picker.click()
  await page.getByRole('option', { name: /^Other branch/ }).click()
  await expect(info).toHaveCount(0)
  await picker.click()
  await page.getByRole('option', { name: /^Collect sources/ }).click()
  await expect(info).toHaveCount(1)
})

for (const [width, language, theme] of [
  [1440, 'zh', 'light'],
  [390, 'en', 'dark'],
] as const) {
  test(`DAG output switching keeps report, copy and independent view state together at ${width}px ${language}`, async ({
    page,
    context,
  }, info) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write'])
    await page.setViewportSize({ width, height: 900 })
    const f = fixture()
    finish(f)
    const secondId = 'second-output-full-stable-id',
      emptyId = 'empty-output-full-stable-id'
    f.run.nodes.push(
      {
        ...f.run.nodes[0],
        id: secondId,
        name: f.run.nodes[0].name,
        execution_id: '00000000-0000-4000-8000-000000000003',
      },
      {
        ...f.run.nodes[0],
        id: emptyId,
        name: 'Empty output',
        state: 'failed',
        execution_id: '00000000-0000-4000-8000-000000000004',
      },
    )
    const first = f.run.result.outputs[0],
      second = {
        ...first,
        node_id: secondId,
        name: first.name,
        result: '# Second output\n\n[Second source](https://example.test/second)',
      },
      empty = {
        node_id: emptyId,
        name: 'Empty output',
        state: 'failed',
        reason: 'Synthetic output failure',
        result: null,
      }
    f.run.result.outputs.push(second, empty)
    const writes = await mock(page, f)
    await page.goto(`/w/${workspace}/runs?run=${id}`)
    await page.evaluate(
      async ({ language, theme }) => {
        const module = await import('/src/i18n.ts')
        await module.setLanguage(language)
        document.documentElement.dataset.theme = theme
      },
      { language, theme },
    )
    const name = language === 'zh' ? '选择输出节点' : 'Select an output node'
    const picker = page.getByRole('combobox', { name, exact: true })
    const result = page.locator('.run-section').filter({ has: picker })
    await expect(picker).toHaveAttribute('title', first.name)
    await expect(result.getByRole('heading', { name: 'Final report', exact: true })).toBeVisible()
    await expect(result.getByRole('heading', { name: 'Second output', exact: true })).toHaveCount(0)
    await result
      .getByRole('button', { name: language === 'zh' ? '原文' : 'Raw', exact: true })
      .click()
    await expect(result.locator('.result-raw')).toContainText('# Final report')
    await picker.press('ArrowDown')
    const menu = page.locator('.dag-node-picker')
    await menu.getByRole('combobox').fill(secondId)
    await expect(menu.getByRole('option')).toHaveCount(1)
    await menu.getByRole('combobox').press('Enter')
    await expect(picker).toBeFocused()
    await expect(result.getByRole('heading', { name: 'Second output', exact: true })).toBeVisible()
    await expect(result.locator('.result-references')).toContainText('Second source')
    await expect(result.locator('.result-raw')).toHaveCount(0)
    await result
      .getByRole('button', { name: language === 'zh' ? '复制' : 'Copy', exact: true })
      .click()
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(second.result)
    await page
      .getByRole('tab', { name: language === 'zh' ? '详情' : 'Details', exact: true })
      .click()
    await page
      .getByRole('tab', { name: language === 'zh' ? '执行' : 'Execution', exact: true })
      .click()
    // Polling may reorder the data, but selection belongs to the stable node ID.
    f.run.result.outputs = [empty, second, first]
    await expect
      .poll(() =>
        page.evaluate(() =>
          document.querySelector('.dag-result-context .choice-selector')?.getAttribute('title'),
        ),
      )
      .toBe(second.name)
    await page.waitForTimeout(2300)
    await expect(result.getByRole('heading', { name: 'Second output', exact: true })).toBeVisible()
    await picker.click()
    await menu.getByRole('combobox').fill(first.node_id)
    await menu.getByRole('combobox').press('End')
    await menu.getByRole('combobox').press('Enter')
    await expect(result.locator('.result-raw')).toContainText('# Final report')
    await picker.click()
    await menu.getByRole('combobox').fill(emptyId)
    await menu.getByRole('combobox').press('Enter')
    await expect(
      result.getByText(language === 'zh' ? '未返回结果' : 'No result returned', { exact: true }),
    ).toBeVisible()
    await expect(result.locator('.dag-result-context .status')).toContainText(
      language === 'zh' ? '失败' : 'Failed',
    )
    await expect(result.locator('.run-result-panel')).toHaveCount(0)
    await expect(result.locator('.run-section-actions').getByRole('button')).toHaveCount(0)
    await result.locator('.run-reason').focus()
    await expect(page.getByRole('tooltip')).toContainText(empty.reason)
    await page.keyboard.press('Escape')
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({ path: info.outputPath('output-switcher.png') })
    expect(writes).toEqual([])
  })
}
