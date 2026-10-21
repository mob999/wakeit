import { test, expect, type Page } from '@playwright/test'
import { workspacePage, mockWorkspace, apiPath, fixtureWorkspace } from './workspace'
const id = '00000000-0000-4000-8000-000000000001'
const child = '00000000-0000-4000-8000-000000000002'
type Options = {
  surface?: 'run' | 'dag' | 'moments'
  href?: string
  result?: string
  preview?: { format: string; content: string; mime?: string }
  pending?: Promise<void>
}
async function fixture(page: Page, role = 'admin', options: Options = {}) {
  const reads: any[] = []
  const href = options.href ?? '/workspace/report.md'
  const config = {
    version: 1,
    id: 'files',
    name: 'File fixture',
    runner: 'Preview device',
    agent: 'pi',
    cwd: '/workspace',
    prompt: 'Synthetic instruction',
    parameters: {},
    enabled: true,
    schedule: null,
    execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
  }
  const run: any = {
    id,
    task_id: 'files',
    config,
    state: 'succeeded',
    source: 'manual',
    created_at: '2026-10-20T01:00:00Z',
    finished_at: '2026-10-20T01:00:01Z',
    result:
      options.result ??
      `[Report](${href})\n\n<a href="file:///workspace/html.md">HTML report</a>\n\n[Windows](C:/data/report.md)\n\n![Local image](/workspace/picture.png)`,
    execution_snapshot: { trace_supported: false, cwd: '/workspace' },
  }
  const execution = { ...run, id: child, parent_run_id: id, node_id: 'report' }
  const node = {
    ...config,
    id: 'report',
    name: 'Report node',
    pre: [],
    state: 'succeeded',
    execution_id: child,
  }
  if (options.surface === 'dag') {
    run.config = { ...config, dag: { nodes: [node] } }
    run.nodes = [node]
    run.result = {
      type: 'dag',
      outputs: [{ node_id: node.id, name: node.name, state: node.state, result: execution.result }],
    }
  }
  await page.route('**/api/v1/**', async (route) => {
    if (await mockWorkspace(route, role)) return
    const path = apiPath(new URL(route.request().url()).pathname)
    if (path === '/session')
      return route.fulfill({ json: { id: 'preview', username: 'preview', role } })
    if (path === '/meta') return route.fulfill({ json: { version: '0.3.0' } })
    if (path === '/settings') return route.fulfill({ json: { timezone: 'UTC' } })
    if (path === '/runners')
      return route.fulfill({
        json: [{ id: 'device', name: 'Preview device', online: true, agents: [] }],
      })
    if (path === '/runs')
      return route.fulfill({ json: { items: [run], total: 1, page: 1, page_size: 20 } })
    if (path === `/runs/${id}`) return route.fulfill({ json: run })
    if (path === `/runs/${id}/nodes/report`) return route.fulfill({ json: { node, execution } })
    if (path === `/runs/${child}`) return route.fulfill({ json: execution })
    if (path === '/moments/days')
      return route.fulfill({
        json: {
          timezone: 'UTC',
          today: '2026-10-20',
          total_runs: 1,
          days: [{ date: '2026-10-20', count: 1 }],
          next_before: null,
        },
      })
    if (path === '/moments')
      return route.fulfill({
        json: {
          timezone: 'UTC',
          date: '2026-10-20',
          items: [
            {
              ...run,
              name: config.name,
              runner: config.runner,
              agent: 'Pi',
              logo: '/agent-logos/pi.svg',
              preview: run.result,
              format: 'markdown',
              has_more: false,
              truncated: false,
            },
          ],
          next_cursor: null,
        },
      })
    if (path.endsWith('/files/preview')) {
      reads.push(route.request().postDataJSON())
      await options.pending
      return route.fulfill({
        json: {
          path: href,
          name: href.split('/').at(-1),
          runner_id: 'device',
          format: 'markdown',
          mime: 'text/plain',
          content: '# Preview\n\n[Nested](./nested.md)',
          size: 33,
          modified_at: null,
          truncated: false,
          ...options.preview,
        },
      })
    }
    if (path.endsWith('/stream'))
      return route.fulfill({ contentType: 'text/event-stream', body: ':fixture\n\n' })
    return route.fulfill({ json: [] })
  })
  await page.goto(
    options.surface === 'moments' ? `/w/${fixtureWorkspace}/` : workspacePage(`/runs?run=${id}`),
  )
  return reads
}

for (const surface of ['report', 'markdown file'] as const) {
  test(`${surface} renders remote Markdown images but does not request local or relative images`, async ({
    page,
  }) => {
    const imageUrl = 'https://images.example.test/fixture.png'
    const content = `![Remote image](${imageUrl})\n\n![Absolute image](/workspace/absolute.png)\n\n![Relative image](./relative.png)`
    const localRequests: string[] = []
    page.on('request', (request) => {
      if (/\/(absolute|relative)\.png(?:\?|$)/.test(request.url()))
        localRequests.push(request.url())
    })
    await page.route(imageUrl, (route) =>
      route.fulfill({
        contentType: 'image/png',
        body: Buffer.from(
          'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl6XzQAAAAASUVORK5CYII=',
          'base64',
        ),
      }),
    )
    const reads = await fixture(
      page,
      'admin',
      surface === 'report' ? { result: content } : { preview: { format: 'markdown', content } },
    )
    if (surface === 'markdown file')
      await page
        .locator('.run-result-panel')
        .getByRole('button', { name: 'Report', exact: true })
        .click()
    const root = surface === 'report' ? page.locator('.run-result-panel') : page.getByRole('dialog')
    const image = root.getByRole('img', { name: 'Remote image', exact: true })
    await expect(image).toBeVisible()
    await expect
      .poll(() => image.evaluate((element: HTMLImageElement) => element.naturalWidth))
      .toBe(1)
    for (const [name, path] of [
      ['Absolute image', '/workspace/absolute.png'],
      ['Relative image', './relative.png'],
    ]) {
      await expect(root.locator(`[data-result-image-href="${path}"]`)).toHaveText(name)
      await expect(root.getByRole('img', { name, exact: true })).toHaveCount(0)
    }
    expect(localRequests).toEqual([])
    expect(reads).toEqual(surface === 'report' ? [] : [{ href: '/workspace/report.md' }])
  })
}

test('local result links preview only on click and preview links do not expand file authorization', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  const reads = await fixture(page)
  await expect(
    page
      .locator('.moment-report, .run-result-panel')
      .getByRole('button', { name: 'Report', exact: true }),
  ).toBeVisible()
  expect(reads).toEqual([])
  expect(await page.locator('img[src="/workspace/picture.png"]').count()).toBe(0)
  await page
    .locator('.moment-report, .run-result-panel')
    .getByRole('button', { name: 'Report', exact: true })
    .click()
  const dialog = page.getByRole('dialog', { name: 'report.md', exact: true })
  await expect(dialog.getByRole('heading', { name: 'Preview', exact: true })).toBeVisible()
  expect(reads).toEqual([{ href: '/workspace/report.md' }])
  await dialog.getByRole('button', { name: 'Nested', exact: true }).click()
  expect(reads).toHaveLength(1)
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe('./nested.md')
  for (const width of [390, 1440])
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize({ width, height: 900 })
      await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
      expect(
        await dialog.evaluate(
          (e) =>
            e.scrollWidth <= e.clientWidth &&
            e.getBoundingClientRect().left >= 7 &&
            e.getBoundingClientRect().right <= innerWidth,
        ),
      ).toBe(true)
      await page.screenshot({ path: `test-results/file-${width}-${theme}.png` })
    }
})
test('file and Windows targets survive sanitization without becoming Gateway paths', async ({
  page,
}) => {
  const reads = await fixture(page)
  await page.getByRole('button', { name: 'HTML report', exact: true }).click()
  await expect(page.getByRole('dialog')).toBeVisible()
  expect(reads[0]).toEqual({ href: 'file:///workspace/html.md' })
  await page.getByRole('dialog').getByRole('button', { name: '关闭', exact: true }).click()
  await page.getByRole('button', { name: 'Windows', exact: true }).click()
  await expect.poll(() => reads.length).toBe(2)
  expect(reads[1]).toEqual({ href: 'C:/data/report.md' })
})
test('Viewer can copy result paths without file IO', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  const reads = await fixture(page, 'viewer')
  await page
    .locator('.moment-report, .run-result-panel')
    .getByRole('button', { name: 'Report', exact: true })
    .click()
  await expect
    .poll(() => page.evaluate(() => navigator.clipboard.readText()))
    .toBe('/workspace/report.md')
  expect(reads).toEqual([])
  await expect(page.getByRole('dialog')).toHaveCount(0)
})

test('file preview stays mounted through Run polling and SSE reconnects', async ({ page }) => {
  let polls = 0,
    streams = 0
  page.on('request', (request) => {
    const path = apiPath(new URL(request.url()).pathname)
    if (path === `/runs/${id}`) polls++
    if (path === `/runs/${id}/stream`) streams++
  })
  const reads = await fixture(page)
  await page
    .locator('.moment-report, .run-result-panel')
    .getByRole('button', { name: 'Report', exact: true })
    .click()
  const dialog = page.getByRole('dialog', { name: 'report.md', exact: true })
  await expect(dialog.getByRole('heading', { name: 'Preview', exact: true })).toBeVisible()
  const original = await dialog.elementHandle()
  const before = polls
  await expect.poll(() => polls, { timeout: 10000 }).toBeGreaterThanOrEqual(before + 3)
  expect(streams).toBeGreaterThan(1)
  await expect(dialog).toBeVisible()
  expect(await original!.evaluate((element) => element.isConnected)).toBe(true)
  expect(reads).toEqual([{ href: '/workspace/report.md' }])
})

test('an in-flight preview survives background updates and retains loaded scroll position', async ({
  page,
}) => {
  let complete!: () => void
  const pending = new Promise<void>((resolve) => {
    complete = resolve
  })
  const content =
    '# Preview\n\n' + Array.from({ length: 100 }, (_, n) => `Paragraph ${n}.`).join('\n\n')
  let polls = 0
  page.on('request', (request) => {
    if (apiPath(new URL(request.url()).pathname) === `/runs/${id}`) polls++
  })
  const reads = await fixture(page, 'admin', { pending, preview: { format: 'markdown', content } })
  try {
    await page
      .locator('.moment-report, .run-result-panel')
      .getByRole('button', { name: 'Report', exact: true })
      .click()
    const dialog = page.getByRole('dialog', { name: 'report.md', exact: true })
    await expect(dialog.getByText('正在加载…', { exact: true })).toBeVisible()
    const before = polls
    await expect.poll(() => polls, { timeout: 10000 }).toBeGreaterThanOrEqual(before + 2)
    await expect(dialog).toBeVisible()
    complete()
    await expect(dialog.getByRole('heading', { name: 'Preview', exact: true })).toBeVisible()
    const body = dialog.locator('.file-preview-content')
    await body.evaluate((element) => {
      element.scrollTop = 200
    })
    const scroll = await body.evaluate((element) => element.scrollTop)
    expect(scroll).toBeGreaterThan(0)
    await expect.poll(() => polls, { timeout: 5000 }).toBeGreaterThanOrEqual(before + 3)
    expect(await body.evaluate((element) => element.scrollTop)).toBe(scroll)
    expect(reads).toHaveLength(1)
  } finally {
    complete()
  }
})

test('closing a pending preview prevents late output from reopening it', async ({ page }) => {
  let complete!: () => void
  const pending = new Promise<void>((resolve) => {
    complete = resolve
  })
  await fixture(page, 'admin', { pending })
  try {
    await page
      .locator('.moment-report, .run-result-panel')
      .getByRole('button', { name: 'Report', exact: true })
      .click()
    const dialog = page.getByRole('dialog', { name: 'report.md', exact: true })
    await expect(dialog.getByText('正在加载…', { exact: true })).toBeVisible()
    await dialog.getByRole('button', { name: '关闭', exact: true }).click()
    complete()
    await page.waitForTimeout(2500)
    await expect(page.getByRole('dialog')).toHaveCount(0)
  } finally {
    complete()
  }
})

for (const item of [
  { surface: 'dag', href: '/workspace/report.json', format: 'json', content: '{"ok":true}' },
  { surface: 'moments', href: '/workspace/notes.txt', format: 'text', content: 'Preview text' },
  {
    surface: 'run',
    href: '/workspace/picture.png',
    format: 'image',
    mime: 'image/png',
    content:
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl6XzQAAAAASUVORK5CYII=',
  },
] as const) {
  test(`${item.surface} preserves ${item.format} preview and its execution source`, async ({
    page,
    context,
  }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write'])
    const reads = await fixture(page, 'admin', {
      surface: item.surface,
      href: item.href,
      preview: item,
    })
    const requests: string[] = []
    page.on('request', (request) => {
      if (request.url().endsWith('/files/preview'))
        requests.push(apiPath(new URL(request.url()).pathname))
    })
    await page
      .locator('.moment-report, .run-result-panel')
      .getByRole('button', { name: 'Report', exact: true })
      .click()
    const dialog = page.getByRole('dialog', { name: item.href.split('/').at(-1), exact: true })
    if (item.format === 'image') {
      await expect(dialog.getByRole('img')).toBeVisible()
      expect(
        await dialog.getByRole('img').evaluate((image: HTMLImageElement) => image.naturalWidth),
      ).toBe(1)
    } else
      await expect(dialog.locator('pre')).toContainText(
        item.format === 'json' ? '"ok": true' : item.content,
      )
    const original = await dialog.elementHandle()
    await page.waitForTimeout(3500)
    await expect(dialog).toBeVisible()
    expect(await original!.evaluate((element) => element.isConnected)).toBe(true)
    expect(requests).toEqual([`/runs/${item.surface === 'dag' ? child : id}/files/preview`])
    expect(reads).toEqual([{ href: item.href }])
    await dialog.getByRole('button', { name: '复制路径', exact: true }).click()
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(item.href)
  })
}

test('workspace permission revocation still closes an open preview', async ({ page }) => {
  await fixture(page)
  await page
    .locator('.moment-report, .run-result-panel')
    .getByRole('button', { name: 'Report', exact: true })
    .click()
  const dialog = page.getByRole('dialog', { name: 'report.md', exact: true })
  await expect(dialog.getByRole('heading', { name: 'Preview', exact: true })).toBeVisible()
  let checked = false
  await page.route(`**/api/v1/workspaces/${fixtureWorkspace}`, (route) => {
    checked = true
    return route.fulfill({
      json: {
        id: fixtureWorkspace,
        name: 'Default',
        role: 'viewer',
        is_default: true,
        is_demo: false,
      },
    })
  })
  await expect.poll(() => checked, { timeout: 15000 }).toBe(true)
  await expect(dialog).toHaveCount(0)
})
