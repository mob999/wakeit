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
  images?: boolean
  mutate?: (run: any) => void
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
      `[Report](${href})\n\n<a href="file:///workspace/html.md">HTML report</a>\n\n[Windows](C:/data/report.md)`,
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
  options.mutate?.(run)
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
      const requested = route.request().postDataJSON().href
      if (options.images && /\.png$/.test(requested))
        return route.fulfill({
          json: {
            path: requested,
            name: requested.split('/').at(-1),
            runner_id: 'device',
            format: 'image',
            mime: 'image/png',
            size: 68,
            truncated: false,
            content:
              'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl6XzQAAAAASUVORK5CYII=',
          },
        })
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
  test(`${surface} renders remote, absolute and relative Markdown images through their correct source`, async ({
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
      surface === 'report'
        ? { result: content, images: true }
        : { preview: { format: 'markdown', content }, images: true },
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
      const local = root.getByRole('img', { name, exact: true })
      await expect(local).toBeVisible()
      await expect
        .poll(() => local.evaluate((element: HTMLImageElement) => element.naturalWidth))
        .toBe(1)
    }
    expect(localRequests).toEqual([])
    expect(reads).toEqual(
      expect.arrayContaining([
        {
          href: '/workspace/absolute.png',
          image: 'thumbnail',
          ...(surface === 'markdown file' ? { document: '/workspace/report.md' } : {}),
        },
        {
          href: './relative.png',
          image: 'thumbnail',
          ...(surface === 'markdown file' ? { document: '/workspace/report.md' } : {}),
        },
      ]),
    )
    expect(reads).toHaveLength(surface === 'report' ? 2 : 3)
    const before = reads.length
    await root.getByRole('button', { name: '放大图片：Absolute image', exact: true }).click()
    const enlarged = page.getByRole('dialog', { name: 'Absolute image', exact: true })
    await expect(enlarged.getByRole('img', { name: 'Absolute image', exact: true })).toBeVisible()
    await enlarged.getByRole('button', { name: '关闭', exact: true }).click()
    await expect(root.getByRole('img', { name: 'Relative image', exact: true })).toBeVisible()
    await expect.poll(() => reads.length).toBe(before + 1)
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
  expect(reads).toEqual([{ href: '/workspace/report.md', image: 'thumbnail' }])
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
  expect(reads[0]).toEqual({ href: 'file:///workspace/html.md', image: 'thumbnail' })
  await page.getByRole('dialog').getByRole('button', { name: '关闭', exact: true }).click()
  await page.getByRole('button', { name: 'Windows', exact: true }).click()
  await expect.poll(() => reads.length).toBe(2)
  expect(reads[1]).toEqual({ href: 'C:/data/report.md', image: 'thumbnail' })
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
  expect(reads).toEqual([{ href: '/workspace/report.md', image: 'thumbnail' }])
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
    const endpoint = `/runs/${item.surface === 'dag' ? child : id}/files/preview`
    expect(requests).toEqual(item.format === 'image' ? [endpoint, endpoint] : [endpoint])
    expect(reads.map((read) => read.image)).toEqual(
      item.format === 'image' ? ['thumbnail', 'original'] : ['thumbnail'],
    )
    expect(reads.every((read) => read.href === item.href)).toBe(true)
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

test('duplicate image references share a read, code examples never read files, and Viewer performs no IO', async ({
  page,
}) => {
  const content =
    '![First](./图%20表.png)\n\n![Second](./图%20表.png)\n\n```md\n![Code](./secret.png)\n```'
  const reads = await fixture(page, 'admin', { result: content, images: true })
  await expect(page.getByRole('img', { name: 'First', exact: true })).toBeVisible()
  await expect(page.getByRole('img', { name: 'Second', exact: true })).toBeVisible()
  expect(reads).toEqual([{ href: './%E5%9B%BE%20%E8%A1%A8.png', image: 'thumbnail' }])
  await page.reload()
  await expect(page.getByRole('img', { name: 'Second', exact: true })).toBeVisible()
  const count = reads.length
  await page.waitForTimeout(2200)
  expect(reads).toHaveLength(count)
  await page.unroute('**/api/v1/**')
  const readonlyReads = await fixture(page, 'viewer', { result: content, images: true })
  await expect(page.locator('[data-result-image-href]').first()).toContainText('First')
  await expect(page.getByRole('img', { name: 'First', exact: true })).toHaveCount(0)
  expect(readonlyReads).toEqual([])
})

test('image errors use Toast and retry, while revoked access removes inline and enlarged content', async ({
  page,
}) => {
  const reads = await fixture(page, 'admin', {
    result: '![Retry image](./retry.png)',
    images: true,
  })
  let fail = true
  await page.route('**/files/preview', async (route) => {
    if (fail)
      return route.fulfill({ status: 400, json: { error: 'Synthetic image is unavailable' } })
    await route.fallback()
  })
  await page.reload()
  const holder = page.locator('[data-result-image-href="./retry.png"]')
  await expect(page.getByRole('alert')).toContainText('Synthetic image is unavailable')
  await expect(holder).toContainText('缩略图加载失败：Synthetic image is unavailable')
  await expect(holder.getByRole('button', { name: '重试', exact: true })).toBeVisible()
  fail = false
  await holder.getByRole('button', { name: '重试', exact: true }).click()
  await expect(holder.getByRole('img')).toBeVisible()
  await holder.getByRole('button', { name: '放大图片：Retry image', exact: true }).click()
  await expect(page.getByRole('dialog', { name: 'Retry image', exact: true })).toBeVisible()
  await page.route(`**/api/v1/workspaces/${fixtureWorkspace}`, (route) =>
    route.fulfill({
      json: {
        id: fixtureWorkspace,
        name: 'Fixture',
        role: 'viewer',
        is_default: true,
        is_demo: false,
      },
    }),
  )
  await expect(page.getByRole('dialog')).toHaveCount(0, { timeout: 15000 })
  await expect(holder.getByRole('img')).toHaveCount(0)
  expect(reads.length).toBeGreaterThan(0)
})

test('automatic image reads are bounded and closing a Markdown preview cancels queued work', async ({
  page,
}) => {
  const content = Array.from({ length: 12 }, (_, n) => `![Image ${n}](./image-${n}.png)`).join(
    '\n\n',
  )
  await fixture(page, 'admin', { preview: { format: 'markdown', content }, images: true })
  let release!: () => void
  const pending = new Promise<void>((resolve) => {
    release = resolve
  })
  let active = 0,
    peak = 0,
    reads = 0
  await page.route('**/files/preview', async (route) => {
    if (!route.request().postDataJSON().document) return route.fallback()
    reads++
    active++
    peak = Math.max(peak, active)
    await pending
    active--
    await route.fallback().catch(() => {})
  })
  try {
    await page
      .locator('.run-result-panel')
      .getByRole('button', { name: 'Report', exact: true })
      .click()
    const dialog = page.getByRole('dialog', { name: 'report.md', exact: true })
    await expect.poll(() => reads).toBe(4)
    await dialog.locator('.file-preview-content').evaluate((element) => {
      element.scrollTop = element.scrollHeight
    })
    await page.waitForTimeout(150)
    expect(peak).toBe(4)
    await dialog.getByRole('button', { name: '关闭', exact: true }).click()
    release()
    await page.waitForTimeout(200)
    expect(reads).toBe(4)
    await expect(page.getByRole('dialog')).toHaveCount(0)
    await expect(page.getByRole('alert')).toHaveCount(0)
  } finally {
    release()
  }
})

test('DAG inline images use actual child execution and remain mounted during polling', async ({
  page,
}) => {
  const reads = await fixture(page, 'admin', {
    surface: 'dag',
    result: '![Node image](./node.png)',
    images: true,
  })
  const endpoints: string[] = []
  page.on('request', (request) => {
    if (request.url().endsWith('/files/preview'))
      endpoints.push(apiPath(new URL(request.url()).pathname))
  })
  await page.reload()
  const image = page.getByRole('img', { name: 'Node image', exact: true })
  await expect(image).toBeVisible()
  const element = await image.elementHandle()
  await page.waitForTimeout(2500)
  expect(await element!.evaluate((node) => node.isConnected)).toBe(true)
  expect(endpoints).toEqual([`/runs/${child}/files/preview`])
  expect(reads.every((read) => read.href === './node.png')).toBe(true)
})

test('linked image keeps its authored link action without nesting buttons', async ({ page }) => {
  await fixture(page, 'admin', {
    result: '[![Thumbnail](./thumb.png)](/workspace/report.md)',
    images: true,
  })
  const report = page.locator('.run-result-panel')
  await expect(report.getByRole('img', { name: 'Thumbnail', exact: true })).toBeVisible()
  await expect(report.locator('button button')).toHaveCount(0)
  await report
    .getByRole('button')
    .filter({ has: page.getByRole('img', { name: 'Thumbnail', exact: true }) })
    .click()
  await expect(page.getByRole('dialog', { name: 'report.md', exact: true })).toContainText(
    'Preview',
  )
})

test('Markdown image cache reuses an unchanged document and refreshes when the document changes', async ({
  page,
}) => {
  const preview = { format: 'markdown', content: '# First document\n\n![Sketch](./sketch.png)' }
  const reads = await fixture(page, 'admin', { preview, images: true })
  const open = () =>
    page.locator('.run-result-panel').getByRole('button', { name: 'Report', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: 'report.md', exact: true })
  await open()
  await expect(dialog.getByRole('img', { name: 'Sketch', exact: true })).toBeVisible()
  expect(reads.filter((read) => read.document)).toHaveLength(1)
  await dialog.getByRole('button', { name: '关闭', exact: true }).click()
  await open()
  await expect(dialog.getByRole('img', { name: 'Sketch', exact: true })).toBeVisible()
  expect(reads.filter((read) => read.document)).toHaveLength(1)
  await dialog.getByRole('button', { name: '关闭', exact: true }).click()
  preview.content = '# Changed document\n\n![Sketch](./sketch.png)'
  await open()
  await expect(dialog.getByRole('img', { name: 'Sketch', exact: true })).toBeVisible()
  expect(reads.filter((read) => read.document)).toHaveLength(2)
})

for (const language of ['zh', 'en']) {
  test(`large inline images load thumbnails first and byte-identical originals only on click (${language})`, async ({
    page,
  }) => {
    await page.addInitScript(
      (language) => localStorage.setItem('wakeit-language', language),
      language,
    )
    await fixture(page, 'admin', { result: '![Large diagram](./large.png)', images: true })
    const pixel = Buffer.from(
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl6XzQAAAAASUVORK5CYII=',
      'base64',
    )
    const bytes = Buffer.concat([pixel, Buffer.alloc(1100000, 19)])
    const revision = 'a'.repeat(64)
    const reads: any[] = []
    await page.route('**/files/preview', (route) => {
      const body = route.request().postDataJSON()
      reads.push(body)
      const offset = body.offset ?? 0,
        end = Math.min(offset + 180000, bytes.length)
      return route.fulfill({
        json: {
          path: '/workspace/large.png',
          name: 'large.png',
          runner_id: 'device',
          format: 'image',
          mime: 'image/png',
          size: bytes.length,
          truncated: false,
          revision,
          image: body.image,
          content:
            body.image === 'thumbnail'
              ? pixel.toString('base64')
              : bytes.subarray(offset, end).toString('base64'),
          ...(body.image === 'original'
            ? { offset, next_offset: end < bytes.length ? end : null }
            : {}),
        },
      })
    })
    await page.reload()
    const image = page.getByRole('img', { name: 'Large diagram', exact: true })
    await expect(image).toBeVisible()
    expect(reads).toEqual([{ href: './large.png', image: 'thumbnail' }])
    await page.locator('.result-image-open').click()
    const dialog = page.getByRole('dialog', { name: 'Large diagram', exact: true })
    await expect(dialog.locator('img')).toHaveAttribute('src', /^blob:/)
    const restored = await dialog
      .locator('img')
      .evaluate(async (img: HTMLImageElement) =>
        Array.from(new Uint8Array(await (await fetch(img.src)).arrayBuffer())),
      )
    expect(Buffer.from(restored)).toEqual(bytes)
    expect(reads[1].image).toBe('original')
    expect(reads[1].revision).toBeUndefined()
    expect(
      reads.slice(2).every((read) => read.image === 'original' && read.revision === revision),
    ).toBe(true)
    for (const width of [390, 1440]) {
      await page.setViewportSize({ width, height: 900 })
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
        true,
      )
    }
    await dialog
      .getByRole('button', { name: language === 'zh' ? '关闭' : 'Close', exact: true })
      .click()
    await expect(page.getByRole('dialog')).toHaveCount(0)
    await expect(image).toBeVisible()
  })
}

test('closing an original load cancels further chunks and changed originals retain the thumbnail with a clear error', async ({
  page,
}) => {
  await fixture(page, 'admin', { result: '![Photo](./photo.png)', images: true })
  const pixel =
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl6XzQAAAAASUVORK5CYII='
  let release!: () => void
  const pending = new Promise<void>((resolve) => {
    release = resolve
  })
  let originals = 0,
    changed = false
  await page.route('**/files/preview', async (route) => {
    const body = route.request().postDataJSON()
    if (body.image === 'original') {
      originals++
      if (changed)
        return route.fulfill({ status: 400, json: { error: 'image changed; reload the preview' } })
      await pending
    }
    await route
      .fulfill({
        json: {
          path: '/workspace/photo.png',
          name: 'photo.png',
          runner_id: 'device',
          format: 'image',
          mime: 'image/png',
          size: 1000000,
          truncated: false,
          revision: 'b'.repeat(64),
          image: body.image,
          content: pixel,
          offset: 0,
          next_offset: 68,
        },
      })
      .catch(() => {})
  })
  await page.reload()
  await expect(page.locator('.result-image-open')).toBeVisible()
  await page.locator('.result-image-open').click()
  const dialog = page.getByRole('dialog', { name: 'Photo', exact: true })
  await expect(dialog).toContainText('正在加载原图…')
  await expect.poll(() => originals).toBe(1)
  await dialog.getByRole('button', { name: '关闭', exact: true }).click()
  release()
  await page.waitForTimeout(200)
  expect(originals).toBe(1)
  changed = true
  await page.locator('.result-image-open').click()
  await expect(dialog).toContainText('原图加载失败：图片在读取期间已变化，请重新打开预览。')
  await expect(dialog.locator('img')).toHaveAttribute('src', /^data:/)
  await expect(page.getByRole('alert')).toContainText('图片在读取期间已变化')
})

for (const surface of ['run', 'dag'] as const) {
  test(`${surface} output references are deduplicated, responsive and use the actual file source`, async ({
    page,
  }, info) => {
    const reads: string[] = []
    page.on('request', (r) => {
      if (r.url().includes('/files/preview')) reads.push(r.url())
    })
    await fixture(page, 'admin', {
      surface,
      result:
        '# Synthetic report\n\n[Report](/workspace/report.md) and [Again](/workspace/report.md).\n\n[Source](https://example.test/source)\n\n`[Not a link](/workspace/hidden.md)`\n\n[Unsafe](javascript:alert)\n\n![Image](./chart.png)',
      images: true,
    })
    const references = page.getByRole('complementary', { name: '引用', exact: true })
    await expect(references).toHaveCount(1)
    await expect(references.locator('.result-reference-list > div')).toHaveCount(3)
    await expect(references).not.toContainText('Not a link')
    await expect(references).not.toContainText('Unsafe')
    const more = references.locator('.result-references-more')
    if (await more.isVisible()) await more.click()
    await expect(references.getByRole('link', { name: 'Source', exact: true })).toHaveAttribute(
      'href',
      'https://example.test/source',
    )
    await references.getByRole('button', { name: '引用: Report', exact: true }).click()
    const dialog = page.getByRole('dialog')
    await expect(dialog).toContainText('Preview')
    expect(reads.at(-1)).toContain(`/runs/${surface === 'dag' ? child : id}/files/preview`)
    await page.waitForTimeout(2300)
    await expect(dialog).toBeVisible()
    await dialog.getByRole('button', { name: '关闭', exact: true }).click()
    await page.screenshot({ path: info.outputPath('references-desktop.png') })
    for (const [width, language, theme] of [
      [390, 'zh', 'dark'],
      [1440, 'en', 'light'],
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
      const aside = page.locator('.result-references')
      await expect(aside.locator('h4')).toHaveText(language === 'zh' ? '引用' : 'References')
      const geometry = await aside.evaluate((el) => {
        const reading = el.previousElementSibling!.getBoundingClientRect(),
          own = el.getBoundingClientRect()
        return { below: own.top >= reading.bottom, beside: own.left >= reading.right }
      })
      expect(width === 390 ? geometry.below : geometry.beside).toBe(true)
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
        true,
      )
      await page.screenshot({ path: info.outputPath(`references-${width}-${language}.png`) })
    }
    await page.getByRole('button', { name: 'Raw', exact: true }).click()
    await expect(page.locator('.result-references')).toHaveCount(0)
    await page.getByRole('button', { name: 'Read', exact: true }).click()
    await expect(page.locator('.result-references')).toHaveCount(1)
  })
}

test('readonly output references copy local paths without reading a device', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  const reads = await fixture(page, 'viewer', { result: '[Report](/workspace/report.md)' })
  await page
    .getByRole('complementary', { name: '引用', exact: true })
    .getByRole('button', { name: '引用: Report', exact: true })
    .click()
  await expect
    .poll(() => page.evaluate(() => navigator.clipboard.readText()))
    .toBe('/workspace/report.md')
  expect(reads).toEqual([])
  await expect(page.getByRole('dialog')).toHaveCount(0)
})

test('multiple DAG outputs keep separate reference lists and execution ownership', async ({
  page,
}) => {
  const other = '00000000-0000-4000-8000-000000000003'
  const requests: string[] = []
  page.on('request', (request) => {
    if (request.url().includes('/files/preview')) requests.push(request.url())
  })
  await fixture(page, 'admin', {
    surface: 'dag',
    result: '[First file](/workspace/first.md)',
    mutate(run) {
      run.nodes.push({ ...run.nodes[0], id: 'other', name: 'Second output', execution_id: other })
      run.config.dag.nodes = run.nodes
      run.result.outputs.push({
        node_id: 'other',
        name: 'Second output',
        state: 'succeeded',
        result: '[Second file](/workspace/second.md)',
      })
    },
  })
  const references = page.locator('.result-references')
  await expect(references).toHaveCount(1)
  await expect(references).toContainText('First file')
  await expect(references).not.toContainText('Second file')
  const picker = page.getByRole('combobox', { name: '选择输出节点', exact: true })
  await picker.click()
  await page.getByRole('option', { name: /^Second output/ }).click()
  await expect(picker).toBeFocused()
  await expect(references).toHaveCount(1)
  await expect(references).toContainText('Second file')
  await expect(references).not.toContainText('First file')
  await references.getByRole('button', { name: '引用: Second file', exact: true }).click()
  await expect(page.getByRole('dialog')).toContainText('Preview')
  expect(requests.at(-1)).toContain(`/runs/${other}/files/preview`)
  await page.getByRole('dialog').getByRole('button', { name: '关闭', exact: true }).click()
  await picker.click()
  await page.getByRole('option', { name: /^Report node/ }).click()
  await references.getByRole('button', { name: '引用: First file', exact: true }).click()
  expect(requests.at(-1)).toContain(`/runs/${child}/files/preview`)
})
