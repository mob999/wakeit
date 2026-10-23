import { languageAction } from './actions'
import { test, expect, type Page } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'

test('both catalogues have matching keys and interpolation parameters', () => {
  const zh = JSON.parse(fs.readFileSync(path.resolve('src/locales/zh.json'), 'utf8'))
  const en = JSON.parse(fs.readFileSync(path.resolve('src/locales/en.json'), 'utf8'))
  expect(Object.keys(en).sort()).toEqual(Object.keys(zh).sort())
  const variables = (value: string) => (value.match(/\{\{[^}]+\}\}/g) ?? []).sort()
  for (const key of Object.keys(zh)) {
    expect(variables(en[key]), key).toEqual(variables(zh[key]))
    expect(en[key], key).not.toMatch(/[\u3400-\u9fff]/)
  }
})

const config = {
  version: 1,
  id: 'fixture-task',
  name: '用户任务 Keep original',
  runner: 'fixture-runner',
  agent: 'pi',
  cwd: '/projects/中文 workspace',
  prompt: '原始指令 Keep this exactly',
  parameters: {},
  enabled: true,
  schedule: { cron: '0 9 * * *', timezone: 'Asia/Shanghai' },
  execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
}
const task = { id: config.id, config, revision: 1, archived_at: null, next_at: null }
const agent = {
  id: 'pi',
  name: 'Pi',
  mode: 'sync',
  digest: 'fixture',
  available: true,
  version: '1.2.0',
  runner: 'fixture-runner',
  runner_id: 'fixture-runner',
  parameters: { model: { type: 'string', label: '模型', enum: [] } },
  models_supported: false,
  session_supported: false,
  trace_supported: true,
}
const runner = {
  id: 'fixture-runner',
  name: 'fixture-runner',
  online: true,
  revoked: false,
  last_seen: '2026-10-15T06:00:00Z',
  agents: [agent],
  directory_roots: ['/projects'],
  info: { os: 'macos', version: '0.1.0', concurrency: 2, active: 0 },
}
const run = {
  id: 'fixture-run',
  task_id: config.id,
  state: 'succeeded',
  config,
  source: 'manual',
  created_at: '2026-10-15T06:00:00Z',
  started_at: '2026-10-15T06:00:00Z',
  finished_at: '2026-10-15T06:01:00Z',
  scheduled_at: '2026-10-15T06:00:00Z',
  result: '# 用户报告\n\nOriginal output.',
  reason: null,
  execution_snapshot: { cwd: config.cwd, parameters: {}, trace_supported: true },
  task_archived: false,
  cancel_requested: false,
  job_id: null,
}
async function fixture(page: Page, loggedIn = true, role = 'admin') {
  const calls: string[] = []
  await page.route('**/api/v1/**', async (route) => {
    const url = new URL(route.request().url())
    const path = url.pathname.replace('/api/v1', '')
    calls.push(`${route.request().method()} ${path}`)
    const send = (json: unknown, status = 200) => route.fulfill({ json, status })
    const paged = (items: unknown[]) => ({ items, total: items.length, page: 1, page_size: 20 })
    if (path === '/session')
      return send(
        loggedIn ? { id: 'user', username: 'fixture-user', role } : { error: 'login required' },
        loggedIn ? 200 : 401,
      )
    if (path === '/meta')
      return send({ version: '0.1.0', public_url: 'https://wakeit.example.test', install: {} })
    if (path === '/settings')
      return send({ timezone: 'Asia/Shanghai', log_retention_days: 7, run_retention_days: 90 })
    if (path === '/timezones') return send(['Asia/Shanghai', 'UTC'])
    if (path === '/agents') return send([agent])
    if (path === '/runners') return send([runner])
    if (path === '/tasks')
      return send(paged(url.searchParams.get('archived') === 'true' ? [] : [task]))
    if (path === `/tasks/${config.id}`) return send(task)
    if (path === '/runs') return send(paged([run]))
    if (path === '/runs/fixture-run') return send(run)
    if (path.endsWith('/events')) return send([])
    if (path.endsWith('/stream'))
      return route.fulfill({ contentType: 'text/event-stream', body: ': connected\n\n' })
    if (path === '/users')
      return send({
        ...paged([
          { id: 'user', username: 'fixture-user', role, active: true, created_at: run.created_at },
        ]),
        active_admins: 1,
      })
    if (path === '/tokens') return send(paged([]))
    if (path === '/moments/days')
      return send({
        timezone: 'Asia/Shanghai',
        today: '2026-10-15',
        total_runs: 1,
        days: [{ date: '2026-10-15', count: 1 }],
        next_before: null,
      })
    if (path === '/moments')
      return send({
        timezone: 'Asia/Shanghai',
        date: '2026-10-15',
        next_cursor: null,
        items: [
          {
            ...run,
            name: config.name,
            runner: runner.name,
            agent: 'Pi',
            preview: run.result,
            format: 'markdown',
            has_more: false,
            truncated: false,
          },
        ],
      })
    return send([])
  })
  return calls
}

test('language changes in place, is beside version, persists, and leaves result data unchanged', async ({
  page,
}) => {
  const calls = await fixture(page)
  await page.goto('/')
  await expect(page.getByRole('link', { name: '动态', exact: true })).toBeVisible()
  await expect(page.getByText('1 次执行', { exact: true })).toBeVisible()
  const button = await languageAction(page, '切换到英文')
  const version = await page.locator('.brand-version').boundingBox()
  const bounds = await page.getByRole('button', { name: '显示偏好' }).boundingBox()
  expect(bounds!.x).toBeGreaterThan(version!.x + version!.width)
  await button.click()
  await expect(page.locator('html')).toHaveAttribute('lang', 'en')
  await expect(page.getByRole('link', { name: 'Moments', exact: true })).toBeVisible()
  await expect(page.getByText('1 run', { exact: true })).toBeVisible()
  await expect(page.getByRole('heading', { name: '用户报告' })).toBeVisible()
  await expect(page.getByText('Original output.')).toBeVisible()
  expect(calls.filter((c) => c.startsWith('POST') || c.startsWith('PUT'))).toEqual([])
  await page.reload()
  await expect(await languageAction(page, 'Switch to Chinese')).toBeVisible()
  await (await languageAction(page, 'Switch to Chinese')).click()
  await expect(page.getByRole('link', { name: '任务', exact: true })).toBeVisible()
})

test('all main pages and account categories render English without translating task content', async ({
  page,
}) => {
  await fixture(page)
  await page.addInitScript(() => localStorage.setItem('wakeit-language', 'en'))
  for (const url of [
    '/tasks?task=fixture-task',
    '/runs?run=fixture-run',
    '/runners?runner=fixture-runner&section=agents',
    '/settings?section=workspace',
    '/settings?section=members',
    '/settings?section=tokens',
    '/settings?section=archived',
  ]) {
    await page.goto(url)
    await expect(await languageAction(page, 'Switch to Chinese')).toBeVisible()
    await expect(page.locator('html')).toHaveAttribute('lang', 'en')
    await expect(async () => {
      const text = await page.locator('body').innerText()
      expect(
        text
          .replaceAll(config.name, '')
          .replaceAll(config.prompt, '')
          .replaceAll(config.cwd, '')
          .replaceAll('用户报告', ''),
      ).not.toMatch(/[\u3400-\u9fff]/)
    }).toPass()
  }
})

test('switching language preserves the task editor draft and its step', async ({ page }) => {
  const calls = await fixture(page)
  await page.goto('/tasks?task=fixture-task')
  await page.getByRole('button', { name: '编辑任务', exact: true }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByLabel('任务名称', { exact: true }).fill('保留草稿')
  await dialog.getByLabel('指令', { exact: true }).fill('Keep the draft 原文')
  await dialog.getByRole('button', { name: '下一步', exact: true }).click()
  await expect(dialog.getByRole('tab', { name: '时间', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  // Exercise the same language API without closing the modal to reach the topbar.
  await page.evaluate(async () => {
    const { setLanguage } = await import('/src/i18n.ts')
    await setLanguage('en')
  })
  await expect(dialog.getByRole('tab', { name: 'Time', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await dialog.getByRole('button', { name: 'Back', exact: true }).click()
  await expect(dialog.getByLabel('Task name', { exact: true })).toHaveValue('保留草稿')
  await expect(dialog.getByLabel('Prompt', { exact: true })).toHaveValue('Keep the draft 原文')
  expect(calls.filter((c) => c.startsWith('PUT'))).toEqual([])
})

test('login and narrow topbar switch languages without overflow in either theme', async ({
  page,
}) => {
  await fixture(page, false)
  await page.goto('/')
  await (await languageAction(page, '切换到英文')).click()
  await expect(page.getByRole('heading', { name: 'Welcome back' })).toBeVisible()
  await expect(page.getByLabel('Username', { exact: true })).toBeVisible()
  await expect(page.getByLabel('Password', { exact: true })).toBeVisible()
  await page.unroute('**/api/v1/**')
  await fixture(page)
  for (const theme of ['light', 'dark']) {
    await page.addInitScript((t) => localStorage.setItem('wakeit-theme', t), theme)
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await expect(await languageAction(page, 'Switch to Chinese')).toBeVisible()
    const button = await (await languageAction(page, 'Switch to Chinese')).boundingBox()
    expect(button!.x).toBeGreaterThanOrEqual(0)
    expect(button!.x + button!.width).toBeLessThanOrEqual(390)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await (await languageAction(page, 'Switch to Chinese')).click()
    await expect(await languageAction(page, '切换到英文')).toBeVisible()
    await (await languageAction(page, '切换到英文')).click()
  }
})

test('a new browser uses its preferred language until the user chooses one', async ({
  browser,
}) => {
  const context = await browser.newContext({ locale: 'en-US' })
  try {
    const page = await context.newPage()
    await fixture(page, false)
    await page.goto('/')
    await expect(page.getByRole('heading', { name: 'Welcome back' })).toBeVisible()
    await (await languageAction(page, 'Switch to Chinese')).click()
    await page.reload()
    await expect(page.getByRole('heading', { name: '欢迎回来' })).toBeVisible()
  } finally {
    await context.close()
  }
})
