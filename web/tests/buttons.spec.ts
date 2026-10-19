import { test, expect } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
import { loginSession } from './session'

const stack = JSON.parse(
  fs.readFileSync(
    process.env.WAKEIT_TEST_STACK ?? path.resolve('../.local/integration-stack.json'),
    'utf8',
  ),
)

test('transparent actions expose tooltips and retain textual form submission', async ({ page }) => {
  await loginSession(page, 'admin', stack.password)
  const create = page
    .locator('.pane-heading')
    .getByRole('button', { name: '新建任务', exact: true })
  await expect(create).toHaveText('')
  const style = await create.evaluate((el) => {
    const s = getComputedStyle(el)
    return {
      width: s.width,
      height: s.height,
      border: s.borderWidth,
      shadow: s.boxShadow,
      background: s.backgroundColor,
    }
  })
  expect(style).toEqual({
    width: '28px',
    height: '28px',
    border: '0px',
    shadow: 'none',
    background: 'rgba(0, 0, 0, 0)',
  })
  await create.focus()
  await expect(page.getByRole('tooltip')).toHaveText('新建任务')
  await page.keyboard.press('Escape')
  await expect(page.getByRole('tooltip')).toHaveCount(0)
  await page.getByRole('heading', { name: '任务', exact: true }).click()
  await create.hover()
  await expect(page.getByRole('tooltip')).toHaveText('新建任务')
  await create.click()
  const dialog = page.getByRole('dialog', { name: '新建任务', exact: true })
  await expect(dialog.getByRole('button', { name: '下一步', exact: true })).toHaveText('下一步')
  await expect(dialog.getByRole('button', { name: '下一步', exact: true })).toHaveClass(/solid/)
  await expect(dialog.getByRole('button', { name: '取消', exact: true })).toHaveClass(/ghost/)
  await dialog.getByRole('button', { name: '关闭', exact: true }).click()
  await page.getByRole('link', { name: '设备', exact: true }).click()
  await page
    .locator('.list-pane')
    .getByRole('button', { name: /^test-device / })
    .click()
  await expect(page.locator('.pane-heading .count')).toHaveCount(0)
  await expect(page.getByRole('tab', { name: 'Agent', exact: true })).toHaveText('Agent')
  await page.getByRole('button', { name: '设备操作', exact: true }).click()
  await expect(page.getByRole('menuitem', { name: '吊销设备', exact: true })).toBeVisible()
  await page.keyboard.press('Escape')
  await page.getByRole('link', { name: '运行', exact: true }).click()
  await expect(page.locator('.pane-heading')).toHaveText('运行')
  await expect(page.locator('.pane-heading svg')).toHaveCount(0)
})

test('title copy actions copy the exact Task and Run IDs without exposing them as headings', async ({
  page,
}) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: {
        writeText: async (value: string) => {
          ;(window as any).copiedValue = value
        },
      },
    })
  })
  await loginSession(page, 'admin', stack.password)
  const task = (await (await page.request.get('/api/v1/tasks?page=1&page_size=1')).json()).items[0]
  await page.goto(`/tasks?task=${task.id}`)
  await expect(page.locator('.header-breadcrumbs [role="heading"]')).toHaveText(task.config.name)
  await expect(page.locator('.detail-heading .eyebrow')).toHaveCount(0)
  const copyTask = page.getByRole('button', { name: '复制 Task ID', exact: true })
  await copyTask.click()
  await expect.poll(() => page.evaluate(() => (window as any).copiedValue)).toBe(task.id)
  await expect(copyTask.locator('.lucide-check')).toBeVisible()
  await expect(page.getByRole('button', { name: '执行历史', exact: true })).toHaveText('执行历史')
  const run = (await (await page.request.get('/api/v1/runs?page=1&page_size=1')).json()).items[0]
  await page.goto(`/runs?run=${run.id}`)
  await expect(page.locator('.header-breadcrumbs [role="heading"]')).toHaveText(run.config.name)
  await page.getByRole('button', { name: '复制 Run ID', exact: true }).click()
  await expect.poll(() => page.evaluate(() => (window as any).copiedValue)).toBe(run.id)
  await expect(page.locator('.detail-heading .status')).toHaveCount(0)
  for (const name of ['任务', '运行', '设备', '设置']) {
    await page.getByRole('link', { name, exact: true }).click()
    await expect(page.locator('.pane-heading .count')).toHaveCount(0)
  }
})

test('the separator occupies no column and supports pointer, keyboard and reset', async ({
  page,
}) => {
  await loginSession(page, 'admin', stack.password)
  const handle = page.getByRole('separator', { name: '调整列表宽度' })
  const seam = async () => {
    const list = await page.locator('.list-pane').boundingBox()
    const detail = await page.locator('.detail-pane').boundingBox()
    expect(detail!.x - list!.x - list!.width).toBeCloseTo(0, 1)
  }
  await seam()
  await expect(handle).toHaveAttribute('aria-orientation', 'vertical')
  const initial = Number(await handle.getAttribute('aria-valuenow'))
  await handle.focus()
  await page.keyboard.press('ArrowRight')
  await expect(handle).toHaveAttribute('aria-valuenow', String(initial + 10))
  const box = await handle.boundingBox()
  expect(box!.width).toBe(8)
  await page.mouse.move(box!.x + 4, box!.y + 120)
  await page.mouse.down()
  await page.mouse.move(box!.x + 84, box!.y + 120)
  await expect(handle).toHaveAttribute('data-resizing', 'true')
  await page.mouse.up()
  await expect(handle).not.toHaveAttribute('data-resizing')
  await seam()
  const moved = await handle.getAttribute('aria-valuenow')
  expect(Number(moved)).toBeGreaterThan(initial + 60)
  await page.reload()
  await expect(handle).toHaveAttribute('aria-valuenow', moved!)
  await handle.dblclick()
  await expect(handle).toHaveAttribute('aria-valuenow', '350')
  await page.setViewportSize({ width: 390, height: 840 })
  await expect(handle).not.toBeVisible()
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy()
})

test('navigation can resize and collapse independently while account and version stay in the header', async ({
  page,
}) => {
  await loginSession(page, 'admin', stack.password)
  const header = page.locator('.topbar')
  await expect(header.locator('.brand')).toHaveText('Wakeit')
  await expect(header.locator('.brand .brand-mark')).toHaveCount(1)
  await expect(header.locator('.top-actions .brand-version')).toHaveText('0.1.0')
  await expect(header.getByRole('button', { name: '用户菜单', exact: true })).toBeVisible()
  await expect(page.locator('.navigation .user-menu')).toHaveCount(0)
  const aligned = async () => {
    const top = await page.locator('.header-navigation').boundingBox()
    const bottom = await page.locator('.navigation').boundingBox()
    expect(top!.x + top!.width).toBeCloseTo(bottom!.x + bottom!.width, 1)
  }
  await aligned()
  const heading = await page.locator('.header-breadcrumbs [role="heading"]').innerText()
  const url = page.url()
  const handle = page.getByRole('separator', { name: '调整导航宽度' })
  await handle.focus()
  await page.keyboard.press('ArrowRight')
  await expect(handle).toHaveAttribute('aria-valuenow', '154')
  const box = await handle.boundingBox()
  await page.mouse.move(box!.x + 4, box!.y + 120)
  await page.mouse.down()
  await page.mouse.move(box!.x + 44, box!.y + 120)
  await page.mouse.up()
  const width = await handle.getAttribute('aria-valuenow')
  expect(Number(width)).toBeGreaterThan(180)
  await header.getByRole('button', { name: '折叠导航', exact: true }).click()
  await expect(header.locator('.brand')).toHaveCount(0)
  await expect(header.getByRole('button', { name: '折叠导航', exact: true })).toHaveCount(0)
  await expect(header.locator('.header-main')).not.toContainText('Wakeit')
  await expect(
    header.getByRole('button', { name: '展开导航', exact: true }).locator('.brand-mark'),
  ).toHaveCount(1)
  await expect(page.getByRole('separator', { name: '调整导航宽度' })).toHaveCount(0)
  await expect
    .poll(async () => (await page.locator('.navigation-frame').boundingBox())!.width)
    .toBe(44)
  await expect(page.locator('.navigation .nav-label').first()).not.toBeVisible()
  await aligned()
  expect(page.url()).toBe(url)
  await expect(page.locator('.header-breadcrumbs [role="heading"]')).toHaveText(heading)
  await page.locator('.navigation').getByRole('link', { name: '运行', exact: true }).hover()
  await expect(page.getByRole('tooltip')).toHaveText('运行')
  await page.reload()
  await expect(header.getByRole('button', { name: '展开导航', exact: true })).toHaveAttribute(
    'aria-expanded',
    'false',
  )
  await header.getByRole('button', { name: '展开导航', exact: true }).click()
  await expect(handle).toHaveAttribute('aria-valuenow', width!)
  await handle.dblclick()
  await expect(handle).toHaveAttribute('aria-valuenow', '144')
  await header.getByRole('button', { name: '用户菜单', exact: true }).click()
  await expect(page.getByRole('menuitem', { name: '退出登录' })).toBeVisible()
  await page.keyboard.press('Escape')
  await page.setViewportSize({ width: 390, height: 840 })
  await expect(header.getByRole('button', { name: '展开导航', exact: true })).toBeVisible()
  await header.getByRole('button', { name: '展开导航', exact: true }).click()
  await expect(page.locator('.navigation-overlay')).toBeVisible()
  await expect(page.locator('.navigation .nav-label').first()).toBeVisible()
  await page.getByRole('button', { name: '关闭导航', exact: true }).click()
  await expect(page.locator('.navigation-overlay')).toHaveCount(0)
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy()
})
