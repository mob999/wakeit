import { test, expect, type Page } from '@playwright/test'
import { mockWorkspace, workspacePage, apiPath } from './workspace'

async function fixture(page: Page) {
  await page.addInitScript(() => {
    class QuietEventSource extends EventTarget {
      onopen: ((event: Event) => void) | null = null
      onerror = null
      constructor(..._args: unknown[]) {
        super()
        setTimeout(() => this.onopen?.(new Event('open')), 0)
      }
      close() {}
    }
    window.EventSource = QuietEventSource as unknown as typeof EventSource
  })
  const config = {
    version: 1,
    id: 'synthetic',
    name: 'Synthetic research task — 多语言长名称与设备识别',
    runner: 'Synthetic workstation with a long name',
    agent: 'pi',
    cwd: '/workspace',
    prompt: 'Synthetic instruction',
    parameters: {},
    schedule: null,
    enabled: true,
    execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
  }
  const tasks = Array.from({ length: 20 }, (_, i) => ({
    id: `task-${i}`,
    config: { ...config, id: `task-${i}`, name: `${i + 1}. ${config.name}` },
    last_run: {
      state: [
        'succeeded',
        'failed',
        'running',
        'warning',
        'unknown',
        'skipped',
        'cancelled',
        'timed_out',
        'interrupted',
        'queued',
        'starting',
      ][i % 11],
    },
    revision: 1,
    next_at: null,
    archived: false,
  }))
  const runs = tasks.map((t, i) => ({
    id: `00000000-0000-4000-8000-${String(i + 1).padStart(12, '0')}`,
    task_id: t.id,
    config: t.config,
    state: t.last_run.state,
    created_at: '2026-10-20T01:00:00Z',
    started_at: '2026-10-20T01:00:00Z',
    finished_at: '2026-10-20T01:00:01Z',
    source: 'manual',
    result: null,
  }))
  const runners = tasks.map((t, i) => ({
    id: `device-${i}`,
    name: t.config.runner + ` ${i + 1}`,
    online: true,
    revoked: false,
    agents: [],
    info: { os: 'macos', active: 1, concurrency: 2 },
    directory_roots: [],
  }))
  await page.route('**/api/v1/**', async (route) => {
    if (await mockWorkspace(route)) return
    const path = apiPath(new URL(route.request().url()).pathname)
    let value: any = []
    if (path === '/session') value = { id: 'synthetic-user', username: 'preview', role: 'admin' }
    else if (path === '/meta') value = { version: '0.3.0' }
    else if (path === '/settings')
      value = { timezone: 'UTC', log_retention_days: 7, run_retention_days: 30 }
    else if (path === '/tasks') value = { items: tasks, total: 40, page: 1, page_size: 20 }
    else if (path.startsWith('/tasks/'))
      value = tasks.find((t) => path.endsWith('/' + t.id)) ?? tasks[0]
    else if (path === '/runs') value = { items: runs, total: 40, page: 1, page_size: 20 }
    else if (/^\/runs\/[^/]+$/.test(path))
      value = runs.find((r) => path.endsWith('/' + r.id)) ?? runs[0]
    else if (path.endsWith('/stream'))
      return route.fulfill({ contentType: 'text/event-stream', body: ':fixture\n\n' })
    else if (path === '/runners') value = runners
    else if (path.startsWith('/runners/')) value = runners[0]
    return route.fulfill({ json: value })
  })
}
test('secondary panes share density, distinct surfaces and stable selected/hover/focus states', async ({
  page,
}) => {
  await fixture(page)
  for (const route of ['tasks', 'runs', 'runners', 'settings']) {
    await page.goto(workspacePage('/' + route))
    const pane = page.locator('.list-pane'),
      rows = pane.locator('.task-row')
    await expect(rows.first()).toBeVisible()
    const measurements = await pane.evaluate((p) => ({
      bg: getComputedStyle(p).backgroundColor,
      heading: getComputedStyle(p.querySelector('h1')!).fontSize,
      header: p.querySelector('.pane-heading')!.getBoundingClientRect().height,
      row: p.querySelector('.task-row')!.getBoundingClientRect().height,
      title: getComputedStyle(p.querySelector('.row-title,.settings-category')!).fontSize,
    }))
    expect(measurements).toEqual({
      bg: 'rgb(250, 250, 250)',
      heading: '12px',
      header: 44,
      row: route === 'settings' ? 36 : route === 'runners' ? 48 : 64,
      title: '12px',
    })
    expect(
      await page
        .locator('#primary-navigation')
        .evaluate((p) => getComputedStyle(p).backgroundColor),
    ).not.toBe(measurements.bg)
    if (route !== 'settings') await rows.first().click()
    await expect(rows.first()).toHaveAttribute('aria-current', 'page')
    await rows.nth(1).hover()
    const selected = await rows.first().evaluate((p) => getComputedStyle(p).backgroundColor)
    expect(await rows.nth(1).evaluate((p) => getComputedStyle(p).backgroundColor)).not.toBe(
      selected,
    )
    await rows.first().focus()
    await page.keyboard.press('Tab')
    await expect(rows.nth(1)).toBeFocused()
    expect(await rows.nth(1).evaluate((p) => getComputedStyle(p).outlineStyle)).not.toBe('none')
    expect(await pane.evaluate((p) => p.scrollWidth <= p.clientWidth)).toBe(true)
  }
})
test('settings width is independent and keyboard/reset controls retain object preferences', async ({
  page,
}) => {
  await fixture(page)
  await page.goto(workspacePage('/tasks'))
  const handle = page.getByRole('separator', { name: '调整列表宽度' })
  await expect(handle).toHaveAttribute('aria-valuenow', '350')
  await handle.press('ArrowRight')
  await expect(handle).toHaveAttribute('aria-valuenow', '360')
  await page.goto(workspacePage('/settings'))
  await expect(handle).toHaveAttribute('aria-valuenow', '240')
  await handle.press('Home')
  await expect(handle).toHaveAttribute('aria-valuenow', '200')
  await page.goto(workspacePage('/tasks'))
  await expect(handle).toHaveAttribute('aria-valuenow', '360')
  await handle.dblclick()
  await expect(handle).toHaveAttribute('aria-valuenow', '350')
})
test('sidebar geometry fits narrow, desktop and wide screens in both languages and themes', async ({
  page,
}) => {
  test.setTimeout(120000)
  await fixture(page)
  for (const width of [390, 1024, 1440, 1920])
    for (const language of ['zh', 'en'])
      for (const theme of ['light', 'dark']) {
        await page.setViewportSize({ width, height: 900 })
        await page.addInitScript(
          ({ language, theme }) => {
            localStorage.setItem('wakeit-language', language)
            localStorage.setItem('wakeit-theme', theme)
          },
          { language, theme },
        )
        await page.goto(workspacePage('/runs'))
        await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
        await expect(page.locator('.list-pane .task-row').first()).toBeVisible()
        expect(
          await page.locator('.list-pane').evaluate((el) => el.scrollWidth <= el.clientWidth),
        ).toBe(true)
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
          true,
        )
        await page.screenshot({ path: `test-results/sidebar-${width}-${language}-${theme}.png` })
      }
})
