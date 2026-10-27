import { test, expect, type Page, type Locator } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
import YAML from 'yaml'
import { loginSession } from './session'
import { selectChoice } from './choices'
import { languageAction } from './actions'

const stack = JSON.parse(
  fs.readFileSync(
    process.env.WAKEIT_TEST_STACK ?? path.resolve('../.local/integration-stack.json'),
    'utf8',
  ),
)
test.skip(!stack.workspaces, 'Requires the isolated workspace acceptance stack')
const workspace = stack.workspaces?.[0]
const password = 'workspace-fixture-password'

function definition(id: string, name = 'Browser template') {
  return {
    version: 1,
    id,
    name,
    description: 'Synthetic browser fixture',
    inputs: {
      type: 'object',
      additionalProperties: false,
      required: ['repositories', 'working_directory'],
      properties: {
        repositories: {
          type: 'array',
          title: '仓库',
          items: {
            type: 'object',
            additionalProperties: false,
            required: ['key', 'text', 'enabled'],
            properties: {
              key: { type: 'string', minLength: 1 },
              text: { type: 'string' },
              enabled: { type: 'boolean' },
            },
          },
        },
        working_directory: { type: 'string', format: 'directory', title: '工作目录' },
        note: { type: 'string', title: '备注', default: 'default note' },
        summarize: { type: 'boolean', default: true },
      },
    },
    bindings: { worker: { title: '检查环境', defaults: {} } },
    task: {
      name: { template: 'Browser [[ inputs.repositories | length ]]' },
      schedule: null,
      execution: { timeout: null, start_grace: '1h', overlap: 'skip', misfire: 'skip' },
      dag: {
        nodes: [
          {
            group: 'review',
            for_each: { expr: 'inputs.repositories' },
            as: 'repo',
            key: 'key',
            when: { expr: 'repo.enabled' },
            node: {
              name: { template: 'Review [[ repo.key ]]' },
              binding: 'worker',
              cwd: { template: '[[ inputs.working_directory ]]' },
              timeout: '20s',
              prompt: {
                parts: [
                  {
                    template: '{"text":[[ repo.text | tojson ]],"sleep":0.05}',
                  },
                ],
              },
            },
          },
          {
            id: 'summary',
            when: { expr: 'inputs.summarize' },
            binding: 'worker',
            name: 'Summary',
            cwd: { template: '[[ inputs.working_directory ]]' },
            timeout: '20s',
            pre: [{ group: 'review', all: true }],
            prompt: {
              parts: [
                { text: 'Reports:\n' },
                { result: { group: 'review', all: true }, separator: '\n---\n' },
              ],
            },
          },
        ],
        results: [
          { node: 'summary', when: { expr: 'inputs.summarize' } },
          { group: 'review', all: true, when: { expr: 'not inputs.summarize' } },
        ],
      },
    },
  }
}

async function openTemplates(page: Page, english = false): Promise<Locator> {
  await page
    .locator('.pane-heading')
    .getByRole('button', { name: english ? 'New task' : '新建任务', exact: true })
    .click()
  await page
    .getByRole('menuitem', { name: english ? 'Create from template' : '从模板创建', exact: true })
    .click()
  const dialog = page.locator('.template-modal')
  await expect(dialog).toBeVisible()
  return dialog
}

async function previewAfter(page: Page, action: () => Promise<void>) {
  const [response] = await Promise.all([
    page.waitForResponse(
      (response) =>
        response.request().method() === 'POST' &&
        /\/task-templates\/[^/]+\/preview$/.test(new URL(response.url()).pathname),
    ),
    action(),
  ])
  expect(response.ok()).toBe(true)
  return response.json()
}

async function addRepository(dialog: Locator, index: number, key: string, enabled = true) {
  await dialog.getByRole('button', { name: '添加一项', exact: true }).click()
  const row = dialog.locator('.template-array-row').nth(index)
  await row.getByLabel(`inputs.repositories[${index}].key`, { exact: true }).fill(key)
  await row
    .getByLabel(`inputs.repositories[${index}].text`, { exact: true })
    .fill(`# ${key[0].toUpperCase()}${key.slice(1)}`)
  await row
    .getByLabel(`inputs.repositories[${index}].enabled`, { exact: true })
    .selectOption(String(enabled))
}

test('authors a dynamic DAG, preserves typed overrides and creates a successful frozen Run', async ({
  page,
}) => {
  await loginSession(page, 'admin', password)
  const dialog = await openTemplates(page)
  const def = definition(`browser-${Date.now()}`)
  await dialog.getByRole('button', { name: '动态 DAG 模板', exact: true }).click()
  await dialog.getByLabel('模板定义', { exact: true }).fill(YAML.stringify(def, { lineWidth: 0 }))
  await dialog.getByRole('button', { name: '保存模板', exact: true }).click()
  await expect(dialog.getByRole('button', { name: '创建并执行', exact: true })).toBeVisible()

  await dialog.getByLabel('工作目录 取值方式', { exact: true }).selectOption('value')
  await dialog.getByLabel('工作目录', { exact: true }).fill(`${stack.root}/project-a`)
  await dialog.getByLabel('仓库 取值方式', { exact: true }).selectOption('value')
  await addRepository(dialog, 0, 'alpha')
  await addRepository(dialog, 1, 'beta')
  await dialog
    .locator('.template-array-row')
    .nth(0)
    .getByRole('button', { name: '下移', exact: true })
    .click()
  await expect(
    dialog.locator('.template-array-row').nth(0).getByLabel('inputs.repositories[0].key'),
  ).toHaveValue('beta')
  await expect(
    dialog.locator('.template-array-row').nth(1).getByLabel('inputs.repositories[1].key'),
  ).toHaveValue('alpha')

  const create = dialog.getByRole('button', { name: '创建并执行', exact: true })
  await dialog.getByLabel('备注 取值方式', { exact: true }).selectOption('unset')
  await expect(create).toBeDisabled()
  await dialog.getByRole('combobox', { name: '检查环境 的设备', exact: true }).click()
  await page
    .getByRole('listbox')
    .getByRole('option', { name: /^same-device(?:\s|$)/ })
    .click()
  const preview = await previewAfter(page, () =>
    selectChoice(page, dialog, '检查环境 的 Agent', 'Mock Events'),
  )
  await expect(dialog.getByText('预览已更新', { exact: true })).toBeVisible()
  expect(preview.issues).toEqual([])
  expect(preview.graph.nodes.map((node: { id: string }) => node.id)).toEqual([
    'review__beta',
    'review__alpha',
    'summary',
  ])
  expect(preview.source_map.review__alpha.input_path).toBe('inputs.repositories[1]')
  expect(preview.resolved_inputs.note).toBeUndefined()
  await expect(create).toBeEnabled()
  const generatedToolbar = dialog.locator('.template-generated > .template-toolbar')
  await expect(generatedToolbar).toContainText('3 个节点')
  await expect(generatedToolbar).toContainText('1 份输出')
  await dialog.getByRole('button', { name: '表格', exact: true }).click()
  await expect(dialog.getByRole('table', { name: '节点', exact: true })).toBeVisible()
  await dialog.getByLabel('搜索节点', { exact: true }).fill('alpha')
  await expect(dialog.locator('.dag-table-entry')).toHaveCount(1)
  await dialog.getByLabel('搜索节点', { exact: true }).fill('')
  await dialog.getByRole('button', { name: '结构图', exact: true }).click()
  await page.screenshot({ path: test.info().outputPath('template-create-light.png') })

  await create.click()
  await expect(page).toHaveURL(/\/runs\?run=/)
  const runId = new URL(page.url()).searchParams.get('run')!
  let final: any
  await expect
    .poll(
      async () => {
        final = await (
          await page.request.get(`/api/v1/workspaces/${workspace}/runs/${runId}`)
        ).json()
        return final.state
      },
      { timeout: 20000 },
    )
    .toBe('succeeded')
  expect(final.result.outputs[0].result).toBe('Reports:\n# Beta\n---\n# Alpha')
  const task = await (
    await page.request.get(`/api/v1/workspaces/${workspace}/tasks/${final.task_id}`)
  ).json()
  expect(task.template_origin.unset).toEqual(['note'])
  expect(task.template_origin.resolved_inputs.note).toBeUndefined()
  expect(task.config.dag.nodes[2].prompt.parts).toEqual([
    { text: 'Reports:\n' },
    { result: 'review__beta' },
    { text: '\n---\n' },
    { result: 'review__alpha' },
  ])
})

test('ignores stale previews and keeps invalid raw drafts while disabling submission', async ({
  page,
}) => {
  await loginSession(page, 'admin', password)
  const def: any = definition(`browser-stale-${Date.now()}`, `Out of order fixture ${Date.now()}`)
  def.inputs.properties.working_directory.default = `${stack.root}/project-a`
  def.inputs.properties.repositories.default = [
    { key: 'initial', text: '# Initial', enabled: true },
  ]
  def.bindings.worker.defaults = { runner: 'same-device', agent: 'mock-events', parameters: {} }
  const saved = await page.request.put(`/api/v1/workspaces/${workspace}/task-templates/${def.id}`, {
    data: { definition: def, expected_revision: null },
    headers: { 'x-wakeit-request': '1' },
  })
  expect(saved.ok(), await saved.text()).toBe(true)

  const dialog = await openTemplates(page)
  await dialog.locator('.template-list-name').filter({ hasText: def.name }).click()
  const create = dialog.getByRole('button', { name: '创建任务', exact: true })
  await expect(create).toBeEnabled()
  await page.route('**/task-templates/*/preview', async (route) => {
    const key = route.request().postDataJSON().values?.repositories?.[0]?.key
    await new Promise((resolve) => setTimeout(resolve, key === 'slow' ? 700 : 50))
    try {
      await route.continue()
    } catch {}
  })
  const key = dialog.getByLabel('inputs.repositories[0].key', { exact: true })
  const slow = page.waitForRequest(
    (request) => request.postDataJSON().values?.repositories?.[0]?.key === 'slow',
  )
  await key.fill('slow')
  await slow
  const fast = page.waitForResponse(
    (response) =>
      response.request().postDataJSON().values?.repositories?.[0]?.key === 'fast' &&
      /\/task-templates\/[^/]+\/preview$/.test(new URL(response.url()).pathname),
  )
  await key.fill('fast')
  await fast
  await expect(dialog.getByText('预览已更新', { exact: true })).toBeVisible()
  await page.waitForTimeout(750)
  await expect(dialog.locator('.template-generated-instruction code')).toHaveText('review__fast')
  await expect(create).toBeEnabled()

  await dialog.getByRole('button', { name: 'YAML', exact: true }).click()
  const raw = dialog.getByLabel('参数覆盖', { exact: true })
  await raw.fill('values: {repositories: [')
  await expect(raw).toHaveValue('values: {repositories: [')
  await expect(create).toBeDisabled()
  await expect(dialog.getByText('预览已过期', { exact: true })).toBeVisible()
})

test('saves an existing Task as a reusable template without creating another Task', async ({
  page,
}) => {
  await loginSession(page, 'admin', password)
  await page.goto('/tasks?task=same-task')
  await page.getByRole('button', { name: '任务操作', exact: true }).click()
  await page.getByRole('menuitem', { name: '保存为模板', exact: true }).click()
  const dialog = page.locator('.template-modal')
  const source = dialog.getByLabel('模板定义', { exact: true })
  await expect(source).not.toHaveValue('')
  const converted = YAML.parse(await source.inputValue())
  converted.name = `Converted fixture ${Date.now()}`
  await source.fill(YAML.stringify(converted, { lineWidth: 0 }))
  await dialog.getByRole('button', { name: '保存模板', exact: true }).click()
  await expect(dialog.getByRole('button', { name: '创建任务', exact: true })).toBeVisible()
  await dialog.locator('.modal-footer').getByRole('button', { name: '关闭', exact: true }).click()

  const chooser = await openTemplates(page)
  await chooser.getByLabel('搜索模板', { exact: true }).fill(converted.name)
  await expect(chooser.locator('.template-list-name')).toHaveCount(1)
  await expect(chooser.locator('.template-list-name')).toContainText(converted.name)
  await expect(chooser.locator('.template-list-row')).toContainText('0 项')
})

test('keeps the English authoring dialog keyboard-accessible and within light and dark viewports', async ({
  page,
}) => {
  await loginSession(page, 'admin', password)
  await (await languageAction(page, '切换到英文')).click()
  await page.route('**/runners', async (route) => {
    const runners = await (await route.fetch()).json()
    await route.fulfill({
      json: [...runners, { ...runners[0], id: 'second-device', name: 'Second device' }],
    })
  })
  const dialog = await openTemplates(page, true)
  await dialog.getByRole('button', { name: 'Dynamic DAG template', exact: true }).click()
  await dialog.getByText('Preview inputs', { exact: true }).click()
  const browse = dialog.getByLabel('Browse on device', { exact: true })
  await expect(browse).toHaveValue('')
  await browse.selectOption({ label: 'same-device' })
  await expect(browse).not.toHaveValue('')
  const fields = dialog.getByRole('button', { name: 'Fields', exact: true })
  await fields.focus()
  await page.keyboard.press('Enter')
  await expect(dialog.getByRole('button', { name: 'YAML / JSON', exact: true })).toBeVisible()
  await dialog.getByRole('button', { name: 'YAML / JSON', exact: true }).click()
  await expect(dialog.getByLabel('Template definition', { exact: true })).toBeVisible()
  await expect(dialog).not.toContainText('taskTemplate.')

  for (const width of [1440, 1024, 390]) {
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize({ width, height: 900 })
      await page.evaluate((theme) => {
        document.documentElement.dataset.theme = theme
      }, theme)
      const bounds = (await dialog.boundingBox())!
      expect(bounds.x).toBeGreaterThanOrEqual(0)
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(width)
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
        true,
      )
      await page.screenshot({ path: test.info().outputPath(`template-${width}-${theme}.png`) })
    }
  }
})
