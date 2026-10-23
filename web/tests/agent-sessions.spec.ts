import { workspacePage, apiPath, mockWorkspace } from './workspace'
import { test, expect, type Page } from '@playwright/test'
import { selectChoice } from './choices'

const runId = '11111111-2222-4333-8444-555555555555'
const runnerId = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee'
const nativeCommand = "cd -- '/workspace' && 'session-agent' 'resume' 'actual-session'"
const agent = {
  id: 'session-agent',
  name: 'Session Agent',
  available: true,
  mode: 'sync',
  digest: 'fixture',
  cancellable: true,
  parameters: { model: { type: 'string', label: '模型', enum: [] } },
  models_supported: false,
  session_supported: true,
  session_list_supported: true,
  runner: 'dev-mac',
  runner_id: runnerId,
}
const configuration = {
  version: 1,
  id: 'session-task',
  name: '会话任务',
  runner: 'dev-mac',
  agent: agent.id,
  cwd: '/workspace',
  prompt: '检查仓库',
  parameters: {},
  session: { id: 'existing-session' },
  enabled: false,
  schedule: null,
  execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
}
async function fixture(
  page: Page,
  options: {
    role?: string
    offline?: boolean
    ready?: boolean
    state?: string
    list?: boolean
    noCommand?: boolean
  } = {},
) {
  const writes: any[] = []
  const inspections: any[] = []
  const resumed: string[] = []
  const sessionQueries: any[] = []
  let failSessionQueries = false
  const run = {
    id: runId,
    task_id: configuration.id,
    config: configuration,
    task_archived: false,
    state: options.state ?? 'succeeded',
    reason: null,
    source: 'manual',
    created_at: '2026-10-15T04:00:00Z',
    scheduled_at: '2026-10-15T04:00:00Z',
    started_at: '2026-10-15T04:00:00Z',
    finished_at: '2026-10-15T05:00:00Z',
    result: '# 报告\n\n任务已完成。',
    cancel_requested: false,
    job_id: null,
    execution_snapshot: {
      parameters: { model: 'actual-model' },
      cwd: '/workspace',
      model: 'actual-model',
    },
    session: {
      id: 'actual-session',
      runner_id: runnerId,
      agent: agent.id,
      cwd: '/workspace',
      ready: options.ready ?? true,
      command: options.noCommand ? null : nativeCommand,
      title: '仓库检查报告',
    },
  }
  const task = {
    id: configuration.id,
    config: configuration,
    revision: 1,
    next_at: null,
    archived_at: null,
  }
  const runner = {
    id: runnerId,
    name: 'dev-mac',
    online: !options.offline,
    revoked: false,
    last_seen: null,
    directory_roots: ['/workspace'],
    info: { os: 'macos', concurrency: 2 },
    agents: [
      { ...agent, session_list_supported: options.list ?? true },
      {
        ...agent,
        id: 'other-agent',
        name: 'Other Agent',
        session_list_supported: options.list ?? true,
      },
    ],
  }
  function list(items: unknown[]) {
    return { items, total: items.length, page: 1, page_size: 20 }
  }
  await page.route('**/api/v1/**', async (route) => {
    if (await mockWorkspace(route, options.role ?? 'admin')) return
    const request = route.request()
    const url = new URL(request.url())
    const path = apiPath(url.pathname)
    if (path === '/session')
      return route.fulfill({
        json: { id: 'fixture-user', username: 'tester', role: options.role ?? 'admin' },
      })
    if (path === '/meta') return route.fulfill({ json: { version: '0.1.0' } })
    if (path === '/settings')
      return route.fulfill({
        json: { timezone: 'Europe/Berlin', log_retention_days: 7, run_retention_days: 90 },
      })
    if (path === '/agents') return route.fulfill({ json: [agent] })
    if (path === '/timezones')
      return route.fulfill({ json: ['Europe/Berlin', 'Asia/Shanghai', 'UTC'] })
    if (path === '/users') return route.fulfill({ json: { ...list([]), active_admins: 1 } })
    if (path === '/tokens') return route.fulfill({ json: list([]) })
    if (path === '/runners')
      return route.fulfill({
        json: [runner, { ...runner, id: 'other-runner', name: 'other-runner' }],
      })
    if (path.endsWith('/sessions/inspect')) {
      inspections.push(request.postDataJSON())
      if (request.postDataJSON().id === 'missing-session')
        return route.fulfill({ status: 404, json: { error: '未找到会话' } })
      return route.fulfill({ json: { version: 1, ...request.postDataJSON(), persisted: true } })
    }
    if (path.endsWith('/sessions/query')) {
      const body = request.postDataJSON()
      sessionQueries.push(body)
      if (failSessionQueries)
        return route.fulfill({ status: 503, json: { error: '会话列表暂不可用' } })
      return route.fulfill({
        json: {
          version: 1,
          sessions: body.q
            ? [{ id: 'searched-session', title: '接口设计', cwd: body.cwd }]
            : body.cursor
              ? [{ id: 'listed-session-2', title: '第二个会话', cwd: body.cwd }]
              : [
                  { id: 'listed-session-1', title: '仓库检查', cwd: body.cwd },
                  { id: '00000000-1111-4222-8333-444444444444', title: null, cwd: body.cwd },
                ],
          next_cursor: !body.q && !body.cursor ? 'page-2' : null,
        },
      })
    }
    if (path.endsWith('/directories')) {
      const current = (url.searchParams.get('path') ?? '/workspace').replace(/\/$/, '')
      return route.fulfill({
        json: {
          path: current,
          parent: null,
          breadcrumbs: [{ name: current, path: current }],
          directories: current === '/workspace' ? [{ name: 'sub', path: '/workspace/sub' }] : [],
          next_cursor: null,
        },
      })
    }
    if (path === '/schedule/preview') return route.fulfill({ json: ['2026-10-16T07:00:00Z'] })
    if (path === '/tasks') return route.fulfill({ json: list([task]) })
    if (path === `/tasks/${configuration.id}`) {
      if (request.method() === 'PUT') {
        writes.push(request.postDataJSON())
        return route.fulfill({ json: task })
      }
      return route.fulfill({ json: task })
    }
    if (path.startsWith('/tasks/') && request.method() === 'PUT') {
      writes.push(request.postDataJSON())
      return route.fulfill({ json: { ...task, config: request.postDataJSON().config } })
    }
    if (path === '/runs') return route.fulfill({ json: list([run]) })
    if (path.endsWith('/terminals'))
      return route.fulfill({
        status: 409,
        json: { error: 'Runner does not support web terminals' },
      })
    if (path === `/runs/${runId}`) {
      resumed.push(path)
      return route.fulfill({ json: run })
    }
    if (path.endsWith('/events')) return route.fulfill({ json: [] })
    if (path.endsWith('/stream'))
      return route.fulfill({ contentType: 'text/event-stream', body: ':ready\n\n' })
    if (path === '/moments/days')
      return route.fulfill({
        json: {
          timezone: 'Asia/Shanghai',
          today: '2026-10-15',
          total_runs: 1,
          days: [{ date: '2026-10-15', count: 1 }],
          next_before: null,
        },
      })
    if (path === '/moments')
      return route.fulfill({
        json: {
          timezone: 'Asia/Shanghai',
          date: '2026-10-15',
          next_cursor: null,
          items: [
            {
              id: runId,
              task_id: task.id,
              name: '会话任务',
              state: run.state,
              reason: null,
              finished_at: run.finished_at,
              runner: 'dev-mac',
              agent: agent.name,
              logo: null,
              preview: run.result,
              format: 'markdown',
              has_more: false,
              truncated: false,
              continuation_available: run.session.ready && !!run.session.command,
            },
          ],
        },
      })
    return route.fulfill({ status: 404, json: { error: '未知测试接口' } })
  })
  return {
    writes,
    inspections,
    resumed,
    run,
    sessionQueries,
    setListError: (value: boolean) => {
      failSessionQueries = value
    },
  }
}
async function edit(page: Page) {
  await page.goto(workspacePage('/tasks'))
  await page.getByRole('button', { name: '编辑', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '编辑任务', exact: true })
  await dialog.locator('summary:visible').filter({ hasText: '高级设置' }).click()
  return dialog
}

async function manualSession(page: Page, dialog: ReturnType<Page['getByRole']>, id: string) {
  await dialog.getByRole('combobox', { name: '会话', exact: true }).click()
  await page.getByRole('button', { name: '输入会话 ID…', exact: true }).click()
  await page.getByLabel('会话 ID', { exact: true }).fill(id)
  await page.getByLabel('会话 ID', { exact: true }).press('Enter')
}

test('one session control auto-inspects, clears errors on selection, and permits offline manual saving', async ({
  page,
}) => {
  const data = await fixture(page)
  const dialog = await edit(page)
  await expect(dialog.getByRole('combobox', { name: '会话', exact: true })).toHaveText(
    'existing-session',
  )
  await expect.poll(() => data.inspections.length).toBe(1)
  await expect(dialog.getByRole('button', { name: '检查会话', exact: true })).toHaveCount(0)
  await expect(dialog.getByLabel('会话 ID', { exact: true })).toHaveCount(0)
  await expect(dialog.getByText('会话可用', { exact: true })).toHaveCount(0)
  const control = dialog.getByRole('combobox', { name: '会话', exact: true })
  await control.click()
  await page.getByRole('option', { name: '每次新建', exact: true }).click()
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 })
    await control.click()
    const selected = page.getByRole('option', { name: '每次新建', exact: true })
    // Radix may reposition the popup after a viewport change. Measure both
    // rectangles in one frame, then wait for the actual alignment invariant.
    await expect
      .poll(() =>
        selected.evaluate((element) => {
          const row = element.getBoundingClientRect()
          const mark = element.querySelector('.choice-option-meta svg')!.getBoundingClientRect()
          return {
            right: Math.round((row.right - mark.right) * 10) / 10,
            centered: Math.abs(mark.y + mark.height / 2 - row.y - row.height / 2) <= 0.5,
          }
        }),
      )
      .toEqual({ right: 8, centered: true })
    await page.screenshot({ path: `test-results/session-check-${width}.png` })
    await page.keyboard.press('Escape')
  }
  await page.setViewportSize({ width: 1440, height: 900 })

  await manualSession(page, dialog, 'missing-session')
  await expect(page.getByRole('alert')).toHaveText('未找到会话')
  await manualSession(page, dialog, 'fixed-session')
  await expect(dialog.getByRole('combobox', { name: '会话', exact: true })).not.toHaveAttribute(
    'aria-describedby',
  )
  await expect.poll(() => data.inspections.length).toBe(3)
  expect(data.inspections.at(-1).id).toBe('fixed-session')
  await page.getByRole('button', { name: '关闭提示', exact: true }).click()
  await dialog.getByRole('button', { name: '下一步', exact: true }).click()
  await dialog.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(dialog).toHaveCount(0)
  expect(data.writes[0].config.session).toEqual({ id: 'fixed-session' })
  await page.unrouteAll({ behavior: 'wait' })
  const offline = await fixture(page, { offline: true })
  const second = await edit(page)
  await manualSession(page, second, 'offline-session')
  await second.getByRole('button', { name: '下一步', exact: true }).click()
  await second.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(second).toHaveCount(0)
  expect(offline.writes[0].config.session.id).toBe('offline-session')
  expect(offline.inspections).toHaveLength(0)
})

test('changing Agent, directory or Runner clears the binding and new tasks default to new sessions', async ({
  page,
}) => {
  await fixture(page)
  const dialog = await edit(page)
  await selectChoice(page, dialog, 'Agent', 'Other Agent')
  await expect(dialog.getByRole('combobox', { name: '会话', exact: true })).toContainText(
    '每次新建',
  )
  await manualSession(page, dialog, 'shared-session')
  await dialog.getByRole('button', { name: '选择目录', exact: true }).click()
  const directories = page.getByRole('dialog', { name: '选择工作目录', exact: true })
  await directories.getByRole('button', { name: '打开 sub', exact: true }).click()
  await directories.getByRole('button', { name: '选择文件夹', exact: true }).click()
  await expect(dialog.getByRole('combobox', { name: '会话', exact: true })).toContainText(
    '每次新建',
  )
  await manualSession(page, dialog, 'shared-session')
  await selectChoice(page, dialog, 'Runner', 'other-runner')
  await selectChoice(page, dialog, 'Agent', agent.name)
  await dialog.locator('summary:visible').filter({ hasText: '高级设置' }).click()
  await expect(dialog.getByRole('combobox', { name: '会话', exact: true })).toContainText(
    '每次新建',
  )
  await dialog.locator('.modal-heading').getByRole('button', { name: '关闭', exact: true }).click()
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  await page.getByRole('menuitem', { name: '任务', exact: true }).click()
  const fresh = page.getByRole('dialog', { name: '新建任务', exact: true })
  await selectChoice(page, fresh, 'Runner', 'dev-mac')
  await selectChoice(page, fresh, 'Agent', agent.name)
  await fresh.locator('summary:visible').filter({ hasText: '高级设置' }).click()
  await expect(fresh.getByRole('combobox', { name: '会话', exact: true })).toContainText('每次新建')
})

test('a completed Run exposes its actual session and pre-fills a new task without copying title or prompt', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  const data = await fixture(page)
  await page.goto(workspacePage(`/runs?run=${runId}`))
  if (page.viewportSize()!.width <= 720)
    await page.getByRole('button', { name: '页面操作', exact: true }).click()
  await page.getByRole('button', { name: '续聊', exact: true }).click()
  const resume = page.getByRole('dialog', { name: '续聊', exact: true })
  await expect(resume).toContainText('dev-mac')
  await expect(resume.locator('.session-terminal')).toBeVisible()
  await resume.getByRole('button', { name: '复制续聊命令', exact: true }).click()
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(nativeCommand)
  await expect(resume).not.toContainText('wakeit run resume')
  await resume.locator('.modal-heading').getByRole('button', { name: '关闭', exact: true }).click()
  await page.getByRole('button', { name: '从此会话创建任务', exact: true }).click()
  const editor = page.getByRole('dialog', { name: '新建任务', exact: true })
  await expect(editor.getByLabel('任务名称', { exact: true })).toBeEmpty()
  await expect(editor.getByLabel('指令', { exact: true })).toBeEmpty()
  await expect(editor.getByRole('combobox', { name: 'Runner', exact: true })).toContainText(
    'dev-mac',
  )
  await expect(editor.getByRole('button', { name: '选择目录', exact: true })).toContainText(
    '/workspace',
  )
  await editor.locator('summary:visible').filter({ hasText: '高级设置' }).click()
  await expect(editor.getByRole('combobox', { name: '会话', exact: true })).toHaveText(
    'actual-session',
  )
  await expect(editor.getByLabel('模型', { exact: true })).toHaveValue('actual-model')
  await editor.getByLabel('任务名称', { exact: true }).fill('后续检查')
  await editor.getByLabel('指令', { exact: true }).fill('继续检查上一次的问题')
  await editor.getByRole('button', { name: '下一步', exact: true }).click()
  await expect(editor.getByLabel('时区', { exact: true })).toHaveValue('Europe/Berlin')
  await editor.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(editor).toHaveCount(0)
  expect(data.writes[0].config.session).toEqual({ id: 'actual-session' })
  expect(data.writes[0].config.parameters.model).toBe('actual-model')
  await page.getByRole('tab', { name: '详情', exact: true }).click()
  await page.getByRole('button', { name: '执行信息', exact: true }).click()
  await expect(page.locator('.run-properties')).toContainText('actual-session')
  await expect(page.locator('.run-properties')).toContainText('仓库检查报告')
})

test('Moments loads the full Run only on continuation and Viewer may copy but cannot create tasks', async ({
  page,
}) => {
  const data = await fixture(page, { role: 'viewer', state: 'failed' })
  await page.goto(workspacePage('/'))
  await expect(page.locator('.moment-card')).toBeVisible()
  expect(data.resumed).toHaveLength(0)
  await page.getByRole('button', { name: '续聊', exact: true }).click()
  await expect(page.getByRole('dialog', { name: '续聊', exact: true })).toBeVisible()
  expect(data.resumed).toHaveLength(1)
  await page.goto(workspacePage(`/runs?run=${runId}`))
  await expect(page.getByRole('button', { name: '续聊', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '从此会话创建任务', exact: true })).toHaveCount(0)
})

test('missing sessions and active Runs do not expose continuation actions', async ({ page }) => {
  await fixture(page, { ready: false })
  await page.goto(workspacePage(`/runs?run=${runId}`))
  await expect(page.getByRole('heading', { name: configuration.name, exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '续聊', exact: true })).toHaveCount(0)
  await page.goto(workspacePage('/'))
  await expect(page.locator('.moment-card')).toBeVisible()
  await expect(page.getByRole('button', { name: '续聊', exact: true })).toHaveCount(0)
  await page.unrouteAll({ behavior: 'wait' })
  await fixture(page, { state: 'running' })
  await page.goto(workspacePage(`/runs?run=${runId}`))
  await expect(page.getByRole('heading', { name: configuration.name, exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '续聊', exact: true })).toHaveCount(0)
  await page.unrouteAll({ behavior: 'wait' })
  await fixture(page, { noCommand: true })
  await page.goto(workspacePage(`/runs?run=${runId}`))
  await expect(page.getByRole('heading', { name: configuration.name, exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '续聊', exact: true })).toHaveCount(0)
  await page.goto(workspacePage('/'))
  await expect(page.locator('.moment-card')).toBeVisible()
  await expect(page.getByRole('button', { name: '续聊', exact: true })).toHaveCount(0)
})

test('Runner session list is searchable and paginated; selection keeps the complete ID for inspection', async ({
  page,
}) => {
  const data = await fixture(page)
  const dialog = await edit(page)
  expect(data.sessionQueries).toHaveLength(0)
  await expect(dialog.getByRole('combobox', { name: '会话', exact: true })).toHaveText(
    'existing-session',
  )
  await dialog.getByRole('combobox', { name: '会话', exact: true }).click()
  await expect(page.getByRole('option', { name: /仓库检查/ })).toBeVisible()
  await expect(page.getByRole('option', { name: /仓库检查/ }).locator('small')).toHaveText(
    'listed-session-1',
  )
  await expect(page.getByRole('option', { name: /未命名会话/ }).locator('small')).toHaveText(
    '00000000-1111-4222-8333-444444444444',
  )
  await page.screenshot({ path: 'test-results/session-list-1440-light.png', fullPage: true })
  await page.setViewportSize({ width: 390, height: 900 })
  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'dark'
  })
  await expect
    .poll(() =>
      page
        .locator('.choice-popover')
        .evaluate((element) => element.getBoundingClientRect().right <= innerWidth),
    )
    .toBeTruthy()
  await page.screenshot({ path: 'test-results/session-list-390-dark.png', fullPage: true })
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'light'
  })
  await page.getByRole('button', { name: '加载更多', exact: true }).click()
  await page.getByRole('option', { name: /第二个会话/ }).click()
  await expect(dialog.getByRole('combobox', { name: '会话', exact: true })).toHaveText('第二个会话')
  await expect(dialog.getByRole('combobox', { name: '会话', exact: true })).toHaveText('第二个会话')
  expect(data.sessionQueries).toEqual([
    { cwd: '/workspace' },
    { cwd: '/workspace', cursor: 'page-2' },
  ])
  await expect.poll(() => data.inspections.at(-1)?.id).toBe('listed-session-2')
  await dialog.getByRole('combobox', { name: '会话', exact: true }).click()
  const search = page.getByRole('combobox', { name: '搜索会话', exact: true })
  await search.fill('设计')
  await expect(page.getByRole('option', { name: /接口设计/ })).toBeVisible()
  await search.dispatchEvent('keydown', { key: 'Enter', code: 'Enter', isComposing: true })
  await expect(search).toBeVisible()
  await expect(dialog.getByRole('combobox', { name: '会话', exact: true })).toHaveText('第二个会话')
  await search.press('Enter')
  await expect(dialog.getByRole('combobox', { name: '会话', exact: true })).toHaveText('接口设计')
  expect(data.sessionQueries.some((query) => query.q === '设计')).toBeTruthy()
})

test('session list failure removes stale choices and preserves manual input without a list interface', async ({
  page,
}) => {
  const data = await fixture(page)
  const dialog = await edit(page)
  await dialog.getByRole('combobox', { name: '会话', exact: true }).click()
  await expect(page.getByRole('option', { name: /仓库检查/ })).toBeVisible()
  data.setListError(true)
  await page.getByRole('button', { name: '刷新会话', exact: true }).click()
  await expect(page.getByRole('alert')).toHaveText('会话列表暂不可用')
  await expect(page.getByRole('option')).toHaveCount(1)
  await expect(page.getByRole('option', { name: '每次新建', exact: true })).toBeVisible()
  await page.keyboard.press('Escape')
  await manualSession(page, dialog, 'manual-after-error')
  await expect(dialog.getByRole('combobox', { name: '会话', exact: true })).toHaveText(
    'manual-after-error',
  )
  await page.unrouteAll({ behavior: 'wait' })
  const manual = await fixture(page, { list: false })
  const next = await edit(page)
  await manualSession(page, next, 'manual-only')
  await expect(next.getByRole('combobox', { name: '会话', exact: true })).toHaveText('manual-only')
  expect(manual.sessionQueries).toHaveLength(0)
})

test('search result groups have equal outer padding without a trailing group margin', async ({
  page,
}) => {
  await fixture(page)
  await page.goto(workspacePage('/tasks'))
  await page.getByRole('button', { name: /搜索/ }).click()
  const results = page.locator('.search-results')
  await expect(results.locator('.search-group').last()).toBeAttached()
  const spacing = await results.evaluate((element) => {
    const style = getComputedStyle(element)
    return {
      top: style.paddingTop,
      bottom: style.paddingBottom,
      left: style.paddingLeft,
      right: style.paddingRight,
      last: getComputedStyle(element.lastElementChild!).marginBottom,
    }
  })
  expect(spacing).toEqual({ top: '12px', bottom: '12px', left: '12px', right: '12px', last: '0px' })
})

test('session controls and continuation keep the workbench style in light, dark and narrow layouts', async ({
  page,
}) => {
  await fixture(page)
  const editor = await edit(page)
  for (const [width, theme] of [
    [1440, 'light'],
    [1920, 'dark'],
    [390, 'dark'],
  ] as const) {
    await page.setViewportSize({ width, height: 900 })
    await page.evaluate((value) => {
      document.documentElement.dataset.theme = value
    }, theme)
    await expect(editor.getByRole('combobox', { name: '会话', exact: true })).toBeVisible()
    expect(
      await editor.evaluate((element) => element.getBoundingClientRect().right <= innerWidth),
    ).toBeTruthy()
    await page.screenshot({
      path: `test-results/session-task-${width}-${theme}.png`,
      fullPage: true,
    })
  }
  await editor.locator('.modal-heading').getByRole('button', { name: '关闭', exact: true }).click()
  await page.goto(workspacePage(`/runs?run=${runId}`))
  if (page.viewportSize()!.width <= 720)
    await page.getByRole('button', { name: '页面操作', exact: true }).click()
  await page.getByRole('button', { name: '续聊', exact: true }).click()
  const resume = page.getByRole('dialog', { name: '续聊', exact: true })
  for (const [width, theme] of [
    [1440, 'light'],
    [1920, 'dark'],
    [390, 'dark'],
  ] as const) {
    await page.setViewportSize({ width, height: 900 })
    await page.evaluate((value) => {
      document.documentElement.dataset.theme = value
    }, theme)
    expect(
      await resume.evaluate((element) => element.scrollWidth <= element.clientWidth),
    ).toBeTruthy()
    await page.screenshot({
      path: `test-results/session-resume-${width}-${theme}.png`,
      fullPage: true,
    })
  }
})

test('late inspection cannot replace the latest selection; resetting removes the binding', async ({
  page,
}) => {
  const data = await fixture(page)
  let release!: () => void
  const gate = new Promise<void>((resolve) => {
    release = resolve
  })
  let slowStarted = false
  await page.route('**/sessions/inspect', async (route) => {
    const body = route.request().postDataJSON()
    if (body.id === 'slow-session') {
      slowStarted = true
      await gate
    }
    await route.fulfill({
      json: { version: 1, ...body, persisted: true, title: `标题 ${body.id}` },
    })
  })
  const dialog = await edit(page)
  const control = dialog.getByRole('combobox', { name: '会话', exact: true })
  await expect(control).toHaveText('标题 existing-session')
  await manualSession(page, dialog, 'slow-session')
  await expect.poll(() => slowStarted).toBe(true)
  await manualSession(page, dialog, 'fast-session')
  await expect(control).toHaveText('标题 fast-session')
  release()
  await expect(control).toHaveText('标题 fast-session')
  await control.click()
  await page.getByRole('option', { name: '每次新建', exact: true }).click()
  await expect(control).toHaveText('每次新建')
  await expect(control).toBeFocused()
  await dialog.getByRole('button', { name: '下一步', exact: true }).click()
  await dialog.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(dialog).toHaveCount(0)
  expect(data.writes[0].config.session).toBeNull()
})

test('manual entry stays in the same narrow popover and Escape leaves the binding unchanged', async ({
  page,
}) => {
  await fixture(page, { list: false })
  const dialog = await edit(page)
  await page.setViewportSize({ width: 390, height: 900 })
  const control = dialog.getByRole('combobox', { name: '会话', exact: true })
  await control.click()
  await page.getByRole('button', { name: '输入会话 ID…', exact: true }).click()
  const input = page.getByLabel('会话 ID', { exact: true })
  await expect(input).toBeFocused()
  await input.fill('discard-this-draft')
  expect(
    await page.locator('.choice-popover').evaluate((e) => {
      const box = e.getBoundingClientRect()
      return box.left >= 0 && box.right <= innerWidth && e.scrollWidth <= e.clientWidth
    }),
  ).toBeTruthy()
  await page.getByRole('button', { name: '返回会话列表', exact: true }).click()
  await expect(page.getByRole('listbox', { name: '可选会话', exact: true })).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(control).toBeFocused()
  await expect(control).toHaveText('existing-session')
  await expect(dialog.getByLabel('会话 ID', { exact: true })).toHaveCount(0)
})

test('page context and actions live only in the topbar across workbenches and narrow layouts', async ({
  page,
}) => {
  await fixture(page)
  for (const [url, label, action] of [
    ['/', '动态', '刷新动态'],
    [`/tasks?task=${configuration.id}`, configuration.name, '任务操作'],
    [`/runs?run=${runId}`, configuration.name, '运行操作'],
    [`/runners?runner=${runnerId}&section=overview`, 'dev-mac', '创建任务'],
    ['/settings?section=workspace', '工作区', null],
    ['/settings?section=members', '成员', null],
    ['/settings?section=tokens', 'API Key', null],
  ] as const) {
    for (const width of [1440, 390]) {
      await page.setViewportSize({ width, height: 900 })
      await page.goto(workspacePage(url))
      const header = page.locator('.topbar')
      const breadcrumbs = header.getByRole('navigation', { name: '层级导航' })
      await expect(breadcrumbs).toContainText(label)
      await expect(
        page.locator('main .breadcrumbs, main .detail-heading, main .moments-heading'),
      ).toHaveCount(0)
      await expect(header.locator('.brand-version')).toHaveText('0.1.0')
      expect(
        await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
      ).toBeTruthy()
      const rect = await breadcrumbs.boundingBox()
      const bounds = await header.boundingBox()
      expect(rect!.y).toBeGreaterThanOrEqual(bounds!.y)
      expect(rect!.y + rect!.height).toBeLessThanOrEqual(bounds!.y + bounds!.height)
      if (action) {
        if (width === 390)
          await header.getByRole('button', { name: '页面操作', exact: true }).click()
        await expect(header.getByRole('button', { name: action, exact: true })).toBeVisible()
        if (width === 390) {
          const actions = header.locator('.page-actions-buttons')
          const box = await actions.boundingBox()
          expect(box!.x).toBeGreaterThanOrEqual(0)
          expect(box!.x + box!.width).toBeLessThanOrEqual(width)
          await page.keyboard.press('Escape')
          // Escape from the trigger or the action toolbar returns focus to the trigger.
          await expect(header.getByRole('button', { name: '页面操作', exact: true })).toBeFocused()
        }
      }
      if (url === '/') {
        const date = await page.locator('.moment-date').boundingBox()
        expect(date!.y - (bounds!.y + bounds!.height)).toBeLessThanOrEqual(17)
      }
      await page.screenshot({
        path: `test-results/header-${url === '/' ? 'moments' : url.split('?')[0].slice(1) + (url.includes('section=') ? '-' + url.split('section=')[1] : '')}-${width}.png`,
        fullPage: true,
      })
    }
  }
})
