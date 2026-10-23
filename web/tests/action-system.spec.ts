import { test, expect, type Page } from '@playwright/test'
import { mockWorkspace, workspacePage, apiPath } from './workspace'
import { languageAction } from './actions'
const id = '00000000-0000-4000-8000-000000000001'
const child = '00000000-0000-4000-8000-000000000002'
const at = '2026-10-22T06:00:00Z'
async function fixture(page: Page, role = 'admin') {
  const writes: string[] = []
  const node = {
    id: 'collect',
    name: 'Collect sources',
    pre: [],
    runner: 'Review device',
    agent: 'pi',
    cwd: '/workspace',
    prompt: 'Synthetic instruction',
    parameters: {},
    timeout: '1h',
  }
  const config = {
    version: 1,
    id: 'review',
    name: 'Synthetic action review',
    enabled: true,
    schedule: null,
    execution: { timeout: null, start_grace: '1h', overlap: 'skip', misfire: 'latest' },
    dag: { nodes: [node] },
  }
  const run = {
    id,
    task_id: 'review',
    config,
    state: 'failed',
    reason: 'Synthetic command failed',
    source: 'manual',
    created_at: at,
    started_at: at,
    finished_at: at,
    result: null,
    nodes: [{ ...node, execution_id: child, state: 'failed', reason: 'Synthetic command failed' }],
  }
  const execution = {
    ...run,
    id: child,
    parent_run_id: id,
    node_id: 'collect',
    nodes: undefined,
    config: { ...config, ...node, dag: undefined },
    execution_snapshot: { trace_supported: true, cwd: '/workspace' },
  }
  const events = [
    {
      id: 1,
      seq: 1,
      run_id: child,
      kind: 'trace',
      created_at: at,
      data: {
        id: 'tool',
        kind: 'tool',
        title: 'Bash',
        status: 'succeeded',
        input: { command: 'pwd' },
        output: '/workspace',
        occurred_at: at,
      },
    },
    {
      id: 2,
      seq: 2,
      run_id: child,
      kind: 'trace_status',
      created_at: at,
      data: { status: 'incomplete', reason: 'event_parse_failed' },
    },
  ]
  await page.route('**/api/v1/**', async (route) => {
    if (await mockWorkspace(route, role)) return
    const path = apiPath(new URL(route.request().url()).pathname)
    if (route.request().method() !== 'GET') writes.push(path)
    if (path === '/session')
      return route.fulfill({ json: { id: 'reviewer', username: 'reviewer', role } })
    if (path === '/meta') return route.fulfill({ json: { version: '0.3.2' } })
    if (path === '/runs') return route.fulfill({ json: { items: [run], total: 1 } })
    if (path === `/runs/${id}`) return route.fulfill({ json: run })
    if (path === `/runs/${child}`) return route.fulfill({ json: execution })
    if (path === `/runs/${id}/nodes/collect`)
      return route.fulfill({
        json: { execution, retry: { action: null, reason: 'Task is archived', affected: [] } },
      })
    if (path.endsWith('/events')) return route.fulfill({ json: events })
    if (path.endsWith('/stream'))
      return route.fulfill({ contentType: 'text/event-stream', body: ': synthetic\n\n' })
    return route.fulfill({ json: [] })
  })
  await page.goto(workspacePage(`/runs?run=${id}`))
  return writes
}

test('disabled node retry retains identity, keyboard description and blocks every activation path', async ({
  page,
}) => {
  const writes = await fixture(page)
  const retry = page.getByRole('button', { name: '重试节点', exact: true })
  await expect(retry).toHaveAttribute('aria-disabled', 'true')
  await retry.focus()
  await expect(retry).toBeFocused()
  await expect(retry).toHaveAccessibleDescription('任务已归档，请先恢复任务。')
  await page.keyboard.press('Enter')
  await page.keyboard.press('Space')
  await retry.dispatchEvent('click')
  expect(writes).toEqual([])
  await (await languageAction(page, '切换到英文')).click()
  await expect(
    page.getByRole('button', { name: 'Retry node', exact: true }),
  ).toHaveAccessibleDescription('Task is archived. Restore it before retrying.')
})

test('diagnostics open by keyboard, dismiss to their trigger and do not turn latest into a toggle', async ({
  page,
}) => {
  await fixture(page)
  const info = page.getByRole('button', { name: '过程记录提示', exact: true })
  await info.focus()
  await page.keyboard.press('Enter')
  const dialog = page.getByRole('dialog', { name: '过程记录提示', exact: true })
  await expect(dialog).toContainText('部分 Agent 事件无法解析')
  await expect(page.getByRole('tooltip')).toHaveCount(0)
  await page.keyboard.press('Escape')
  await expect(info).toBeFocused()
  const latest = page.getByRole('button', { name: '回到最新', exact: true })
  await expect(latest).not.toHaveAttribute('aria-pressed')
  await page.getByRole('button', { name: '全部收起', exact: true }).click()
  await expect(latest).toHaveAccessibleDescription(/已暂停/)
  await latest.click()
  await expect(latest).toHaveAccessibleDescription(/正在跟随/)
})

test('more menus preserve exact copy values, announce completion and keep hover/open actions transparent', async ({
  page,
}) => {
  await page.addInitScript(() =>
    Object.defineProperty(navigator, 'clipboard', {
      value: {
        writeText: async (value: string) => {
          ;(window as any).copiedAction = value
        },
      },
    }),
  )
  await fixture(page)
  const more = page.getByRole('button', { name: '运行操作', exact: true })
  await more.hover()
  await expect(more).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)')
  await more.click()
  await expect(more).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)')
  await expect(page.getByRole('menuitem', { name: '重新运行整图', exact: true })).toBeVisible()
  await page.getByRole('menuitem', { name: '复制 Run ID', exact: true }).click()
  await expect.poll(() => page.evaluate(() => (window as any).copiedAction)).toBe(id)
  await expect(page.locator('[aria-live=polite]').filter({ hasText: '复制 Run ID' })).toContainText(
    '已完成',
  )
  await expect(more).toBeFocused()
})

test.describe('touch and narrow actions', () => {
  test.use({ hasTouch: true, viewport: { width: 390, height: 844 } })
  test('uses distinct 44px targets and a textual page menu without closing its nested menu', async ({
    page,
  }) => {
    await fixture(page)
    const toggle = page.getByRole('button', { name: '页面操作', exact: true })
    await expect(toggle).toHaveCSS('width', '44px')
    await expect(toggle).toHaveCSS('height', '44px')
    await toggle.click()
    const more = page.getByRole('button', { name: '运行操作', exact: true })
    await expect(more.locator('.action-narrow-label')).toBeVisible()
    await more.click()
    await expect(page.getByRole('menuitem', { name: '复制 Run ID', exact: true })).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(more).toBeFocused()
    await expect(toggle).toHaveAttribute('aria-expanded', 'true')
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
    ).toBeTruthy()
  })
})
