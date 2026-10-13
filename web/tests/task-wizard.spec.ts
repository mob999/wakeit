import { selectChoice } from './choices'
import { test, expect, type Page } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
import { loginSession } from './session'
const stack = JSON.parse(
  fs.readFileSync(
    process.env.WAKEIT_TEST_STACK ?? path.resolve('../.local/integration-stack.json'),
    'utf8',
  ),
)
async function open(page: Page) {
  await loginSession(page, 'admin', stack.password)
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  return page.getByRole('dialog', { name: '新建任务', exact: true })
}
async function fill(page: Page, dialog: any) {
  await dialog.getByLabel('任务名称', { exact: true }).fill('每日检查')
  await dialog.getByLabel('指令', { exact: true }).fill('完整草稿\n中文')
  await selectChoice(page, dialog, 'Runner', 'test-device')
  await selectChoice(page, dialog, 'Agent', 'mock-sync')
  await dialog.getByRole('button', { name: '选择目录', exact: true }).click()
  const picker = page.getByRole('dialog', { name: '选择工作目录', exact: true })
  await picker.getByRole('button', { name: '选择文件夹', exact: true }).click()
  await expect(dialog.getByRole('combobox', { name: '模型', exact: true })).toBeEnabled()
  await dialog.getByRole('combobox', { name: '模型', exact: true }).click()
  await page.getByRole('option', { name: /Model B/ }).click()
}
test('Task validates before Time, never writes on Next, and preserves models and schedule drafts', async ({
  page,
}) => {
  const dialog = await open(page)
  let puts = 0
  page.on('request', (r) => {
    if (r.method() === 'PUT' && r.url().includes('/api/v1/tasks/')) puts++
  })
  await expect(dialog.getByRole('button', { name: '保存任务', exact: true })).toHaveCount(0)
  await expect(dialog.getByRole('button', { name: 'YAML', exact: true })).toHaveCount(0)
  await dialog.getByRole('tab', { name: '时间', exact: true }).click()
  await expect(dialog.getByRole('alert')).toHaveText('请填写任务名称')
  await expect(dialog.getByRole('tab', { name: '任务', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await fill(page, dialog)
  await dialog.getByRole('button', { name: '下一步', exact: true }).click()
  expect(puts).toBe(0)
  await expect(dialog.getByLabel('Cron 表达式', { exact: true })).toHaveValue('0 9 * * *')
  await expect(dialog.locator('.preview small')).toHaveCount(5)
  await dialog.getByLabel('Cron 表达式', { exact: true }).fill('30 10 * * *')
  await dialog.getByRole('button', { name: '上一步', exact: true }).click()
  await expect(dialog.getByLabel('指令', { exact: true })).toHaveValue('完整草稿\n中文')
  await expect(dialog.getByRole('combobox', { name: '模型', exact: true })).toContainText('Model B')
  await dialog.getByRole('tab', { name: '时间', exact: true }).click()
  await expect(dialog.getByLabel('Cron 表达式', { exact: true })).toHaveValue('30 10 * * *')
  await page.screenshot({ path: 'test-results/task-time-cron-light.png', fullPage: true })
  for (const type of ['every', 'at', 'manual']) {
    await selectChoice(page, dialog, '类型', type)
    if (type === 'manual') {
      await expect(dialog.getByLabel('时区', { exact: true })).toHaveCount(0)
      await expect(dialog.locator('.preview')).toHaveCount(0)
    } else {
      await expect(
        dialog.getByLabel(type === 'every' ? '间隔' : '执行时间', { exact: true }),
      ).toBeVisible()
      await expect(dialog.locator('.preview small')).not.toHaveCount(0)
    }
  }
  expect(puts).toBe(0)
  for (const [width, theme] of [
    [1440, 'light'],
    [390, 'light'],
    [1440, 'dark'],
  ] as const) {
    await page.setViewportSize({ width, height: 900 })
    if (theme === 'dark')
      await page.evaluate(() => {
        document.documentElement.dataset.theme = 'dark'
      })
    expect(
      await dialog.evaluate((el) => el.getBoundingClientRect().right <= innerWidth),
    ).toBeTruthy()
    await page.screenshot({ path: `test-results/task-time-${width}-${theme}.png`, fullPage: true })
  }
})
test('save errors and revision conflict retain the complete draft and fixed suffix', async ({
  page,
}) => {
  const dialog = await open(page)
  await fill(page, dialog)
  await dialog.getByRole('button', { name: '下一步', exact: true }).click()
  await selectChoice(page, dialog, '类型', 'manual')
  const writes: any[] = []
  await page.route('**/api/v1/tasks/*', async (route) => {
    if (route.request().method() !== 'PUT') return route.continue()
    writes.push(route.request().postDataJSON())
    await route.fulfill({
      status: writes.length === 1 ? 503 : 409,
      json: { error: writes.length === 1 ? '保存失败' : '版本冲突' },
    })
  })
  await dialog.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(dialog.getByRole('alert')).toHaveText('保存失败')
  await dialog.getByRole('button', { name: '上一步', exact: true }).click()
  await expect(dialog.getByRole('combobox', { name: '模型', exact: true })).toContainText('Model B')
  await dialog.getByLabel('任务名称', { exact: true }).fill('Daily check')
  await dialog.getByRole('button', { name: '下一步', exact: true }).click()
  await dialog.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(dialog.getByRole('alert')).toHaveText('版本冲突')
  expect(writes[0].config.id).toMatch(/^mei-ri-jian-cha-[a-f0-9]{10}$/)
  expect(writes[1].config.id).toBe(`Daily-check-${writes[0].config.id.slice(-10)}`)
  expect(writes[1].config.parameters.model).toBe('model-b')
  expect(writes[1].config.schedule).toBeNull()
  await expect(dialog.getByRole('tab', { name: '时间', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
})

test('saving disables duplicate submit, tab switching and closing until the response arrives', async ({
  page,
}) => {
  const dialog = await open(page)
  await fill(page, dialog)
  await dialog.getByRole('button', { name: '下一步', exact: true }).click()
  await selectChoice(page, dialog, '类型', 'manual')
  let release: () => void = () => {}
  const pending = new Promise<void>((resolve) => {
    release = resolve
  })
  let writes = 0
  await page.route('**/api/v1/tasks/*', async (route) => {
    if (route.request().method() !== 'PUT') return route.continue()
    writes++
    await pending
    await route.fulfill({ status: 503, json: { error: '稍后重试' } })
  })
  await dialog.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(dialog.getByRole('button', { name: '正在保存…', exact: true })).toBeDisabled()
  await expect(dialog.getByRole('tab', { name: '任务', exact: true })).toBeDisabled()
  await expect(dialog.getByRole('button', { name: '上一步', exact: true })).toBeDisabled()
  await dialog.getByRole('button', { name: '关闭', exact: true }).click()
  await expect(dialog).toBeVisible()
  expect(writes).toBe(1)
  release()
  await expect(dialog.getByRole('alert')).toHaveText('稍后重试')
  await expect(dialog.getByRole('button', { name: '保存任务', exact: true })).toBeEnabled()
})
