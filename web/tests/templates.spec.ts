import { test, expect, type Page, type Locator } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
import YAML from 'yaml'
import { randomUUID } from 'node:crypto'
import { loginSession } from './session'
import { selectChoice } from './choices'

const stack = JSON.parse(
  fs.readFileSync(
    process.env.WAKEIT_TEST_STACK ?? path.resolve('../.local/integration-stack.json'),
    'utf8',
  ),
)
test.skip(!stack.workspaces, 'Requires the isolated workspace acceptance stack')
const workspace = stack.workspaces?.[0]
const password = 'workspace-fixture-password'
const apiPath = (path: string) => `/api/v1/workspaces/${workspace}${path}`

function dagDefinition(name: string) {
  return {
    version: 1,
    id: `preset-${randomUUID()}`,
    name: `${name} ${randomUUID().slice(0, 8)}`,
    description: 'Synthetic preset workbench fixture',
    inputs: {
      type: 'object',
      additionalProperties: false,
      required: ['repositories', 'working_directory'],
      properties: {
        repositories: {
          type: 'array',
          title: '仓库',
          default: [
            { key: 'alpha', text: '# Alpha', enabled: true },
            { key: 'beta', text: '# Beta', enabled: true },
          ],
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
        working_directory: {
          type: 'string',
          format: 'directory',
          title: '工作目录',
          default: `${stack.root}/project-a`,
        },
        note: { type: 'string', title: '备注', default: 'default note' },
        summarize: { type: 'boolean', default: true },
      },
    },
    bindings: {
      worker: {
        title: '检查环境',
        defaults: { runner: 'same-device', agent: 'mock-events', parameters: {} },
      },
    },
    task: {
      name: { template: 'Review [[ inputs.repositories | length ]] repositories' },
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
                parts: [{ template: '{"text":[[ repo.text | tojson ]],"sleep":0.05}' }],
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

function ordinaryDefinition(name: string) {
  return {
    version: 1,
    id: `preset-${randomUUID()}`,
    name: `${name} ${randomUUID().slice(0, 8)}`,
    description: 'Synthetic ordinary task fixture',
    inputs: {
      type: 'object',
      additionalProperties: false,
      required: ['repository', 'working_directory'],
      properties: {
        repository: {
          type: 'string',
          format: 'repository-url',
          default: 'https://example.com/repo',
        },
        working_directory: {
          type: 'string',
          format: 'directory',
          default: `${stack.root}/project-a`,
        },
        note: { type: 'string', default: 'default note' },
      },
    },
    bindings: {
      worker: {
        title: '检查环境',
        defaults: { runner: 'same-device', agent: 'mock-events', parameters: {} },
      },
    },
    task: {
      name: { template: 'Review [[ inputs.repository ]]' },
      schedule: null,
      execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
      binding: 'worker',
      cwd: { template: '[[ inputs.working_directory ]]' },
      prompt: { template: '{"text":"Review [[ inputs.repository ]] [[ inputs.note ]]"}' },
    },
  }
}

async function seed(page: Page, definition: any) {
  const response = await page.request.put(apiPath(`/task-templates/${definition.id}`), {
    data: { definition, expected_revision: null },
    headers: { 'x-wakeit-request': '1' },
  })
  expect(response.ok(), await response.text()).toBe(true)
  return definition
}

async function archiveExistingPresets(page: Page) {
  const result = await (
    await page.request.get(apiPath('/task-templates?archived=false&page_size=100'))
  ).json()
  for (const row of result.items) {
    const response = await page.request.post(apiPath(`/task-templates/${row.id}/archive`), {
      data: { expected_revision: row.revision },
      headers: { 'x-wakeit-request': '1' },
    })
    expect(response.ok()).toBe(true)
  }
}

async function chooseTemplate(page: Page, name: string, scope: Page | Locator = page) {
  await scope.getByRole('combobox', { name: '模板（可选）', exact: true }).click()
  await page.getByRole('listbox').getByRole('option', { name, exact: true }).click()
}

async function openOrdinaryEditor(page: Page) {
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  await expect(page.getByRole('menuitem', { name: '从模板创建' })).toHaveCount(0)
  await page.getByRole('menuitem', { name: '任务', exact: true }).click()
  return page.getByRole('dialog', { name: '新建任务', exact: true })
}

async function setLanguage(page: Page, language: 'en' | 'zh') {
  await page.evaluate(async (language) => {
    const { setLanguage } = await import('/src/i18n.ts')
    await setLanguage(language)
  }, language)
}

test('Preset workbench manages definitions, previews, archives and bilingual details without writes', async ({
  page,
}) => {
  await loginSession(page, 'admin', password)
  await archiveExistingPresets(page)
  await page.goto(`/w/${workspace}/presets`)
  await expect(page.locator('.pane-heading h1')).toHaveText('预设')
  await expect(page.getByText('暂无预设', { exact: true }).first()).toBeVisible()
  const before = (await (await page.request.get(apiPath('/tasks'))).json()).length
  await page.getByRole('button', { name: '新建预设', exact: true }).click()
  await page.getByRole('menuitem', { name: '动态 DAG 模板', exact: true }).click()
  const def = dagDefinition('Repository preset')
  await page.getByLabel('模板定义', { exact: true }).fill(YAML.stringify(def, { lineWidth: 0 }))
  await page.getByRole('button', { name: '保存模板', exact: true }).click()
  await expect(page).toHaveURL(new RegExp(`/presets\\?template=${def.id}`))
  await expect(page.locator('.header-breadcrumbs')).toContainText(def.name)
  await expect(page.locator('.preset-row')).toContainText('DAG')
  await expect(page.locator('.preset-row')).toContainText('4 项')
  await expect(page.locator('.preset-overview')).toContainText('r1')

  await page.getByRole('tab', { name: '生成预览', exact: true }).click()
  await expect(page.locator('.template-generated .dag-structure')).toBeVisible()
  const previewToolbar = page.locator('.template-preview-status')
  await expect(previewToolbar).toContainText('3 个节点')
  await expect(previewToolbar).toContainText('1 份输出')
  await page.getByRole('button', { name: '表格', exact: true }).click()
  await page.getByLabel('搜索节点', { exact: true }).fill('alpha')
  await expect(page.locator('.dag-table-entry')).toHaveCount(1)
  await page.getByRole('button', { name: '结构图', exact: true }).click()
  expect((await (await page.request.get(apiPath('/tasks'))).json()).length).toBe(before)

  await page.getByRole('tab', { name: '定义', exact: true }).click()
  const source = page.getByLabel('模板定义', { exact: true })
  const edited = { ...def, name: 'Repository preset v2' }
  await source.fill(YAML.stringify(edited, { lineWidth: 0 }))
  await page.getByRole('button', { name: '保存模板', exact: true }).click()
  await expect(page.locator('.preset-overview')).toContainText('r2')
  await page.screenshot({ path: test.info().outputPath('preset-overview-light.png') })
  await page.getByRole('tab', { name: '定义', exact: true }).click()
  await source.fill(YAML.stringify(edited, { lineWidth: 0 }) + '\n# unsaved\n')
  await page.getByRole('link', { name: '任务', exact: true }).click()
  const leave = page.getByRole('dialog', { name: '有未保存的更改', exact: true })
  await expect(leave).toBeVisible()
  await leave.getByRole('button', { name: '继续编辑', exact: true }).click()
  await expect(source).toHaveValue(/# unsaved/)
  await page.getByRole('link', { name: '任务', exact: true }).click()
  await leave.getByRole('button', { name: '放弃更改', exact: true }).click()
  await expect(page).toHaveURL(/\/tasks/)

  await page.goto(`/w/${workspace}/presets?template=${def.id}&section=overview`)
  await page.getByRole('button', { name: '预设操作', exact: true }).click()
  await page.getByRole('menuitem', { name: '归档', exact: true }).click()
  await expect(page.locator('.preset-overview')).toContainText('已归档')
  await page.getByRole('button', { name: '预设操作', exact: true }).click()
  await page.getByRole('menuitem', { name: '恢复', exact: true }).click()
  await expect(page.locator('.preset-overview')).not.toContainText('已归档')

  await page.getByRole('tab', { name: '生成预览', exact: true }).click()
  await setLanguage(page, 'en')
  await expect(page.getByRole('link', { name: 'Presets', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: 'New preset', exact: true })).toBeVisible()
  const preview = page.getByRole('tab', { name: 'Generated preview', exact: true })
  await preview.focus()
  await page.keyboard.press('End')
  await expect(page.getByRole('tab', { name: 'Definition', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await page.keyboard.press('Home')
  await expect(page.getByRole('tab', { name: 'Overview', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await page.keyboard.press('ArrowRight')
  await expect(preview).toHaveAttribute('aria-selected', 'true')
  await expect(page.getByText('Preview is current', { exact: true })).toBeVisible()
  await expect(page.locator('.dag-graph-node').first()).toBeVisible()

  for (const width of [1440, 1024, 390]) {
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize({ width, height: 900 })
      await page.evaluate((theme) => {
        document.documentElement.dataset.theme = theme
      }, theme)
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
        true,
      )
      await page.screenshot({ path: test.info().outputPath(`preset-${width}-${theme}.png`) })
    }
  }
})

test('ordinary Task creation offers an optional template inside the existing editor and preserves origin', async ({
  page,
}) => {
  await loginSession(page, 'admin', password)
  const def = await seed(page, ordinaryDefinition('Inline ordinary preset'))
  const dialog = await openOrdinaryEditor(page)
  await expect(dialog.getByRole('combobox', { name: '模板（可选）', exact: true })).toContainText(
    '不使用模板',
  )
  await chooseTemplate(page, def.name, dialog)
  await dialog.getByLabel('任务名称', { exact: true }).fill('Inline ordinary task')
  await setLanguage(page, 'en')
  const englishDialog = page.getByRole('dialog')
  await expect(englishDialog.getByLabel('Task name', { exact: true })).toHaveValue(
    'Inline ordinary task',
  )
  await expect(
    englishDialog.getByRole('combobox', { name: 'Template (optional)', exact: true }),
  ).toContainText(def.name)
  await englishDialog.getByRole('button', { name: 'Next', exact: true }).click()
  await englishDialog.getByRole('button', { name: 'Save task', exact: true }).click()
  await expect(englishDialog).toHaveCount(0)
  const tasks = await (await page.request.get(apiPath('/tasks'))).json()
  const created = tasks.find((task: any) => task.config.name === 'Inline ordinary task')
  expect(created.template_origin.template_id).toBe(def.id)
  expect(JSON.parse(created.config.prompt)).toEqual({
    text: 'Review https://example.com/repo default note',
  })
  expect(created.config.session).toBeUndefined()
})

test('DAG creation replaces the canvas only when a template is selected and saves the generated graph', async ({
  page,
}) => {
  await loginSession(page, 'admin', password)
  const def = await seed(page, dagDefinition('Inline DAG preset'))
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  await page.getByRole('menuitem', { name: 'DAG 任务', exact: true }).click()
  await chooseTemplate(page, def.name, page.locator('.dag-template-picker'))
  await expect(page.locator('.dag-template-instance .dag-structure')).toBeVisible()
  await expect(page.locator('.dag-canvas-mount')).toBeHidden()
  await page.getByLabel('任务名称', { exact: true }).fill('Inline DAG task')
  await page.getByRole('button', { name: '下一步', exact: true }).click()
  await page.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(page).toHaveURL(/\/tasks\?task=/)
  const tasks = await (await page.request.get(apiPath('/tasks'))).json()
  const created = tasks.find((task: any) => task.config.name === 'Inline DAG task')
  expect(created.template_origin.template_id).toBe(def.id)
  expect(created.config.dag.nodes.map((node: any) => node.id)).toEqual([
    'review__alpha',
    'review__beta',
    'summary',
  ])
  expect(created.config.dag.results).toEqual(['summary'])
})

test('instance previews ignore stale responses, preserve invalid drafts and require an explicit directory Runner', async ({
  page,
}) => {
  const def: any = dagDefinition('Preview state preset')
  def.bindings.worker.defaults = {}
  await loginSession(page, 'admin', password)
  await seed(page, def)
  await page.route('**/runners', async (route) => {
    const runners = await (await route.fetch()).json()
    await route.fulfill({
      json: [...runners, { ...runners[0], id: 'second-device', name: 'Second device' }],
    })
  })
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  await page.getByRole('menuitem', { name: 'DAG 任务', exact: true }).click()
  await chooseTemplate(page, def.name, page.locator('.dag-template-picker'))
  await expect(page.getByLabel('浏览设备', { exact: true })).toHaveValue('')
  const before = (await (await page.request.get(apiPath('/tasks'))).json()).length
  await page.route('**/task-templates/*/preview', async (route) => {
    const key = route.request().postDataJSON().values?.repositories?.[0]?.key
    await new Promise((resolve) => setTimeout(resolve, key === 'slow' ? 700 : 50))
    try {
      await route.continue()
    } catch {}
  })
  const key = page.getByLabel('inputs.repositories[0].key', { exact: true })
  const slow = page.waitForRequest(
    (request) =>
      request.method() === 'POST' &&
      request.url().includes('/task-templates/') &&
      request.postDataJSON().values?.repositories?.[0]?.key === 'slow',
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
  await expect(page.getByText('预览已更新', { exact: true })).toBeVisible()
  await page.waitForTimeout(750)
  await expect(page.locator('.template-node-heading code')).toHaveText('review__fast')

  await page.getByRole('button', { name: 'YAML', exact: true }).click()
  const raw = page.getByLabel('参数覆盖', { exact: true })
  await raw.fill('values: {repositories: [')
  await page.getByRole('button', { name: '下一步', exact: true }).click()
  await page.getByRole('button', { name: '保存任务', exact: true }).click()
  await expect(raw).toHaveValue('values: {repositories: [')
  await expect(page.getByText('预览已过期', { exact: true }).first()).toBeVisible()
  expect((await (await page.request.get(apiPath('/tasks'))).json()).length).toBe(before)
})

test('saving an existing Task opens the preset definition editor instead of a dialog', async ({
  page,
}) => {
  await loginSession(page, 'admin', password)
  await page.goto(`/w/${workspace}/tasks?task=same-task`)
  await page.getByRole('button', { name: '任务操作', exact: true }).click()
  await page.getByRole('menuitem', { name: '保存为模板', exact: true }).click()
  await expect(page).toHaveURL(/\/presets\?new=task&source=same-task/)
  const source = page.getByLabel('模板定义', { exact: true })
  await expect(source).not.toHaveValue('')
  const converted = YAML.parse(await source.inputValue())
  converted.name = `Converted preset ${Date.now()}`
  await source.fill(YAML.stringify(converted, { lineWidth: 0 }))
  await page.getByRole('button', { name: '保存模板', exact: true }).click()
  await expect(page.locator('.preset-row').filter({ hasText: converted.name })).toContainText(
    converted.name,
  )
  const record = await (await page.request.get(apiPath(`/task-templates/${converted.id}`))).json()
  expect(record.definition.task.binding).toBe('environment-1')
  expect(typeof record.definition.task.prompt).toBe('string')
})
