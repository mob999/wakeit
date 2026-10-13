import { test, expect, type Page } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
import { loginSession } from './session'
import { selectChoice } from './choices'
const stack = JSON.parse(
  fs.readFileSync(
    process.env.WAKEIT_TEST_STACK ?? path.resolve('../.local/integration-stack.json'),
    'utf8',
  ),
)
async function picker(page: Page, windows = false) {
  await loginSession(page, 'admin', stack.password)
  const roots = windows ? ['C:\\Users\\开发者'] : ['/home/developer']
  const root = roots[0],
    sep = windows ? '\\' : '/'
  const join = (name: string) => root + sep + name
  const requests: string[] = []
  let removed = false
  let delayed = false
  const files = new Map<string, string[]>([
    [
      root,
      [
        '.hidden',
        'Documents',
        'projects',
        '中文 目录',
        ...Array.from({ length: 205 }, (_, i) => `item-${String(i).padStart(3, '0')}`),
        'z-target',
      ],
    ],
    [join('projects'), ['demo project']],
    [join('projects') + sep + 'demo project', []],
    [join('Documents'), []],
    [join('中文 目录'), []],
    [join('.hidden'), []],
    [join('z-target'), []],
  ])
  await page.route('**/api/v1/runners', (route) =>
    route.fulfill({
      json: [
        {
          id: 'directory-device',
          name: '目录设备',
          online: true,
          revoked: false,
          directory_roots: roots,
          agents: [],
          info: { os: windows ? 'windows' : 'linux' },
        },
      ],
    }),
  )
  await page.route('**/api/v1/runners/directory-device/directories**', async (route) => {
    const url = new URL(route.request().url())
    const raw = url.searchParams.get('path')
    const folder = raw?.replace(/[\\/]$/, '') ?? null
    requests.push(url.search)
    if (!folder)
      return route.fulfill({
        json: {
          path: null,
          parent: null,
          breadcrumbs: [],
          directories: roots.map((p) => ({ name: p, path: p })),
          next_cursor: null,
        },
      })
    if (delayed) await new Promise((resolve) => setTimeout(resolve, 600))
    if (!folder.startsWith(root))
      return route.fulfill({ status: 400, json: { error: '目录超出允许范围' } })
    if (removed && folder === join('Documents'))
      return route.fulfill({ status: 400, json: { error: '目录不存在' } })
    if (!files.has(folder)) return route.fulfill({ status: 400, json: { error: '目录不存在' } })
    const offset = Number(url.searchParams.get('cursor') ?? 0)
    const names = files.get(folder)!
    return route.fulfill({
      json: {
        path: folder,
        parent: folder === root ? null : folder.slice(0, folder.lastIndexOf(sep)),
        breadcrumbs: [],
        directories: names
          .slice(offset, offset + 200)
          .map((name) => ({ name, path: folder + sep + name })),
        next_cursor: offset + 200 < names.length ? String(offset + 200) : null,
      },
    })
  })
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  const task = page.getByRole('dialog', { name: '新建任务', exact: true })
  await task.getByLabel('任务名称', { exact: true }).fill('目录草稿')
  await selectChoice(page, task, 'Runner', '目录设备')
  await task.getByRole('button', { name: '选择目录', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '选择工作目录', exact: true })
  await expect(dialog.getByRole('option', { name: 'Documents', exact: true })).toBeVisible()
  return {
    task,
    dialog,
    root,
    sep,
    join,
    requests,
    remove: () => {
      removed = true
    },
    delay: () => {
      delayed = true
    },
  }
}
test('starts inside root, selects a folder without drilling, and reopens the current selection', async ({
  page,
}) => {
  const { task, dialog, root, join, requests } = await picker(page)
  await expect(dialog.getByRole('combobox', { name: '目录路径' })).toHaveValue(root + '/')
  await expect(dialog.getByRole('combobox', { name: '目录路径' })).toBeFocused()
  expect(requests.every((r) => r.includes('path='))).toBeTruthy()
  await dialog.getByRole('combobox', { name: '目录路径' }).press('ArrowUp')
  await expect(dialog.getByRole('option').last()).toHaveAttribute('aria-selected', 'true')
  await dialog.getByRole('option', { name: 'Documents', exact: true }).click()
  await expect(dialog.locator('output')).toHaveText(join('Documents'))
  await dialog.getByRole('button', { name: '选择文件夹', exact: true }).click()
  await expect(dialog).not.toBeVisible()
  await expect(task.getByRole('button', { name: '选择目录', exact: true })).toContainText(
    join('Documents'),
  )
  await task.getByRole('button', { name: '选择目录', exact: true }).click()
  await expect(dialog.getByRole('combobox', { name: '目录路径' })).toHaveValue(
    join('Documents') + '/',
  )
  await expect(dialog).toContainText('没有子目录')
})
test('double click and keyboard completion navigate; parent/root and Escape preserve the task draft', async ({
  page,
}) => {
  const { task, dialog, root, join } = await picker(page)
  await dialog.getByRole('option', { name: 'projects', exact: true }).dblclick()
  const input = dialog.getByRole('combobox', { name: '目录路径' })
  await expect(input).toHaveValue(join('projects') + '/')
  await input.fill('demo')
  await expect(dialog.getByRole('option', { name: 'demo project', exact: true })).toBeVisible()
  await input.press('Tab')
  await expect(input).toHaveValue(join('projects') + '/demo project/')
  await expect(input).toBeFocused()
  await input.press('Alt+ArrowUp')
  await expect(input).toHaveValue(join('projects') + '/')
  await dialog.getByRole('button', { name: '允许的根目录' }).click()
  await dialog.getByRole('button', { name: `打开 ${root}`, exact: true }).click()
  await expect(input).toHaveValue(root + '/')
  await input.press('Escape')
  await expect(dialog).not.toBeVisible()
  await expect(task.getByLabel('任务名称', { exact: true })).toHaveValue('目录草稿')
  await expect(task.getByRole('button', { name: '选择目录', exact: true })).not.toContainText(root)
})
test('searches past page 200, toggles dot directories, and accepts pasted Unicode paths without losing Enter', async ({
  page,
}) => {
  const { task, dialog, join, requests } = await picker(page)
  await expect(dialog.getByRole('option', { name: '.hidden', exact: true })).toHaveCount(0)
  await dialog.getByRole('button', { name: '显示隐藏目录' }).click()
  await expect(dialog.getByRole('option', { name: '.hidden', exact: true })).toBeVisible()
  const input = dialog.getByRole('combobox', { name: '目录路径' })
  await input.fill('z-target')
  await expect(dialog.getByRole('option', { name: 'z-target', exact: true })).toBeVisible()
  expect(requests.some((r) => r.includes('cursor=200'))).toBeTruthy()
  await input.fill(join('中文 目录'))
  await input.press('Enter')
  await expect(input).toHaveValue(join('中文 目录') + '/')
  await input.press('Control+Enter')
  await expect(dialog).not.toBeVisible()
  await expect(task.getByRole('button', { name: '选择目录', exact: true })).toContainText(
    join('中文 目录'),
  )
})
test('revalidates removed selections, rejects outside paths and recovers without choosing stale data', async ({
  page,
}) => {
  const { dialog, root, remove } = await picker(page)
  await dialog.getByRole('option', { name: 'Documents', exact: true }).click()
  remove()
  await dialog.getByRole('button', { name: '选择文件夹', exact: true }).click()
  await expect(dialog.getByRole('alert')).toContainText('目录不存在')
  const input = dialog.getByRole('combobox', { name: '目录路径' })
  await input.fill('/outside/')
  await expect(dialog.getByRole('alert')).toContainText('目录超出允许范围')
  await expect(dialog.getByRole('button', { name: '选择文件夹', exact: true })).toBeDisabled()
  await dialog.getByRole('button', { name: '允许的根目录' }).click()
  await dialog.getByRole('button', { name: `打开 ${root}`, exact: true }).click()
  await expect(dialog.getByRole('button', { name: '选择文件夹', exact: true })).toBeEnabled()
})
test('closing while a selection is being checked does not apply it later', async ({ page }) => {
  const { task, dialog, root, delay } = await picker(page)
  await dialog.getByRole('option', { name: 'Documents', exact: true }).click()
  delay()
  await dialog.getByRole('button', { name: '选择文件夹', exact: true }).click()
  await expect(dialog.getByRole('button', { name: '选择文件夹', exact: true })).toBeDisabled()
  await dialog.getByRole('button', { name: '关闭', exact: true }).click()
  await page.waitForTimeout(800)
  await expect(task.getByRole('button', { name: '选择目录', exact: true })).not.toContainText(root)
})
test('Windows paths remain native, with compact light/dark layouts and a thin input focus', async ({
  page,
}) => {
  const { dialog, root, join, sep } = await picker(page, true)
  const input = dialog.getByRole('combobox', { name: '目录路径' })
  await input.fill(join('中文 目录'))
  await input.press('Enter')
  await expect(input).toHaveValue(join('中文 目录') + sep)
  await dialog.getByRole('button', { name: '上一级' }).click()
  await expect(input).toHaveValue(root + sep)
  for (const width of [1440, 2200, 390]) {
    await page.setViewportSize({ width, height: 900 })
    for (const theme of ['light', 'dark']) {
      await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
      expect(await dialog.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBeTruthy()
      await input.focus()
      expect(await input.evaluate((e) => getComputedStyle(e).outlineStyle)).toBe('none')
      await page.screenshot({ path: `test-results/directory-picker-${width}-${theme}.png` })
    }
  }
})
