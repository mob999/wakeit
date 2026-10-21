import { test, expect, type Page } from '@playwright/test'
import { workspacePage, mockWorkspace, apiPath } from './workspace'
const id = '00000000-0000-4000-8000-000000000001'
const nativeCommand = 'cd -- /workspace && pi --session native-session'
async function fixture(page: Page, role = 'admin') {
  const changes: string[] = []
  const inputs: any[] = []
  const config = {
    version: 1,
    id: 'test',
    name: 'Terminal fixture',
    runner: 'Preview device',
    agent: 'pi',
    cwd: '/workspace',
    prompt: 'Synthetic instruction',
    parameters: {},
    enabled: true,
    schedule: null,
    execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
  }
  const run = {
    id,
    task_id: config.id,
    config,
    state: 'succeeded',
    source: 'manual',
    created_at: '2026-10-20T01:00:00Z',
    finished_at: '2026-10-20T01:00:01Z',
    result: 'Synthetic result',
    session: {
      id: 'native-session',
      ready: true,
      runner_id: 'device',
      agent: 'pi',
      cwd: '/workspace',
      command: nativeCommand,
    },
  }
  await page.route('**/api/v1/**', async (route) => {
    if (await mockWorkspace(route, role)) return
    const path = apiPath(new URL(route.request().url()).pathname)
    if (path === '/session')
      return route.fulfill({ json: { id: 'user', username: 'preview', role } })
    if (path === '/meta') return route.fulfill({ json: { version: '0.3.0' } })
    if (path === '/settings') return route.fulfill({ json: { timezone: 'UTC' } })
    if (path === '/runners')
      return route.fulfill({
        json: [
          {
            id: 'device',
            name: 'Preview device',
            online: true,
            agents: [],
            info: { web_terminal: 1 },
          },
        ],
      })
    if (path === '/runs')
      return route.fulfill({ json: { items: [run], total: 1, page: 1, page_size: 20 } })
    if (path === `/runs/${id}`) return route.fulfill({ json: run })
    if (path.endsWith('/events')) return route.fulfill({ json: [] })
    if (path.endsWith('/stream'))
      return route.fulfill({ contentType: 'text/event-stream', body: ':ready\n\n' })
    if (path.endsWith('/terminals')) {
      changes.push('create')
      return route.fulfill({ json: { id: 'terminal', state: 'created' } })
    }
    if (path.endsWith('/attach')) {
      changes.push('attach')
      return route.fulfill({ json: { ticket: 'one-use-ticket', expires_in: 30 } })
    }
    if (path.endsWith('/close')) {
      changes.push('close')
      return route.fulfill({ json: { id: 'terminal', state: 'closed' } })
    }
    return route.fulfill({ json: [] })
  })
  await page.routeWebSocket('**/terminals/*/stream', (ws) => {
    ws.onMessage((raw) => {
      const message = JSON.parse(String(raw))
      inputs.push(message)
      if (message.ticket)
        ws.send(
          JSON.stringify({
            kind: 'snapshot',
            data: { seq: 0, screen: btoa('Native terminal\r\n'), state: { state: 'running' } },
          }),
        )
      if (message.kind === 'input') {
        ws.send(JSON.stringify({ kind: 'ack', data: { seq: message.data.seq } }))
        ws.send(
          JSON.stringify({ kind: 'output', data: { data: btoa('\x1b[2J\x1b[HNative Agent') } }),
        )
      }
    })
  })
  await page.goto(workspacePage(`/runs?run=${id}`))
  await page.getByRole('button', { name: '续聊', exact: true }).click()
  return { changes, inputs, dialog: page.getByRole('dialog', { name: '续聊', exact: true }) }
}

test('native terminal keeps keys, command copy, focus, and explicit close semantics', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  const { changes, inputs, dialog } = await fixture(page)
  await expect(dialog.locator('.session-terminal')).toHaveAttribute('data-status', 'connected')
  expect(changes).toEqual(['create', 'attach'])
  await dialog.locator('.xterm-helper-textarea').focus()
  await page.keyboard.press('Escape')
  await page.keyboard.press('Tab')
  await page.keyboard.press('ArrowUp')
  await page.keyboard.press('Control+c')
  await expect.poll(() => inputs.filter((x) => x.kind === 'input').length).toBeGreaterThan(0)
  const keys = inputs
    .filter((x) => x.kind === 'input')
    .map((x) => Buffer.from(x.data.data, 'base64').toString())
    .join('')
  expect(keys).toContain('\x1b')
  expect(keys).toContain('\t')
  expect(keys).toContain('\x03')
  await expect(dialog).toBeVisible()
  await page.mouse.click(3, 3)
  await expect(dialog).toBeVisible()
  await dialog.getByRole('button', { name: '复制续聊命令' }).click()
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(nativeCommand)
  await expect(dialog.locator('.modal-footer, .properties, .session-resume-command')).toHaveCount(0)
  await dialog.getByRole('button', { name: '关闭', exact: true }).click()
  await expect(dialog).toHaveCount(0)
  expect(changes.at(-1)).toBe('close')
  await expect(page.getByRole('button', { name: '续聊', exact: true })).toBeFocused()
})

test('Viewer gets terminal command copy without creating or attaching a PTY', async ({ page }) => {
  const { changes, dialog } = await fixture(page, 'viewer')
  await expect(dialog.locator('.session-terminal')).toHaveAttribute('data-status', 'copyOnly')
  expect(changes).toEqual([])
  await dialog.getByRole('button', { name: '关闭', exact: true }).click()
  expect(changes).toEqual([])
})

test('terminal fits 390px/desktop and both themes using synthetic data', async ({ page }) => {
  const { dialog } = await fixture(page)
  await expect(dialog.locator('.session-terminal')).toHaveAttribute('data-status', 'connected')
  for (const width of [390, 1440])
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize({ width, height: 900 })
      await page.evaluate((theme) => {
        document.documentElement.dataset.theme = theme
      }, theme)
      await expect
        .poll(() =>
          dialog.evaluate(
            (e) =>
              e.getBoundingClientRect().right <= innerWidth &&
              e.getBoundingClientRect().left >= 7 &&
              e.scrollWidth <= e.clientWidth,
          ),
        )
        .toBeTruthy()
      await page.screenshot({ path: `test-results/terminal-${width}-${theme}.png` })
    }
  await dialog.getByRole('button', { name: '关闭', exact: true }).click()
})
