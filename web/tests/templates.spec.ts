import { test, expect, type Page, type Locator } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
import { randomUUID } from 'node:crypto'
import { loginSession } from './session'
import { selectChoice } from './choices'

const stack = JSON.parse(
  fs.readFileSync(
    process.env.WAKEIT_TEST_STACK ?? path.resolve('../.local/integration-stack.json'),
    'utf8',
  ),
)
test.use({ channel: 'msedge' })
test.skip(!stack.workspaces || !stack.cwd, 'Requires the isolated creation acceptance stack')
const workspace = stack.workspaces?.[0]
const password = stack.password ?? 'workspace-fixture-password'
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
          default: stack.cwd,
        },
        note: { type: 'string', title: '备注', default: 'default note' },
        summarize: { type: 'boolean', default: true },
      },
    },
    bindings: {
      worker: {
        title: '检查环境',
        defaults: { runner: stack.runner_name, agent: 'mock-events', parameters: {} },
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
        repository: { type: 'string', title: '仓库地址', format: 'repository-url', minLength: 1 },
        ratio: { type: 'number', title: '比例', default: 0 },
        flag: { type: 'boolean', title: '启用附注', default: false },
        nullable: { type: ['string', 'null'], title: '可空参数', default: null },
        working_directory: {
          type: 'string',
          format: 'directory',
          default: stack.cwd,
        },
        note: { type: 'string', default: 'default note' },
      },
    },
    bindings: {
      worker: {
        title: '检查环境',
        defaults: { runner: stack.runner_name, agent: 'mock-events', parameters: {} },
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

async function openCreation(page: Page, tab?: string) {
  await page.goto(`/w/${workspace}/tasks`)
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '新建任务', exact: true })
  if (tab) await dialog.getByRole('tab', { name: tab, exact: true }).click()
  return dialog
}
async function choosePreset(page: Page, dialog: Locator, name: string) {
  const search = dialog.getByRole('searchbox', { name: '搜索预设' })
  if (!(await search.isVisible()))
    await dialog.getByRole('button', { name: '切换预设', exact: true }).click()
  await search.fill(name)
  await dialog.locator('.preset-choice').filter({ hasText: name }).click()
}
async function setLanguage(page: Page, language: 'en' | 'zh') {
  await page.evaluate(async (language) => {
    const { setLanguage } = await import('/src/i18n.ts')
    await setLanguage(language)
  }, language)
}
async function discard(page: Page) {
  const confirm = page.getByRole('dialog', { name: '有未保存的更改', exact: true })
  if (await confirm.isVisible()) await confirm.getByRole('button', { name: '放弃更改' }).click()
}

test('one plus opens Task/DAG/Preset tabs, preserves drafts and confirms cancellation once', async ({
  page,
}) => {
  await loginSession(page, stack.username, password)
  const dialog = await openCreation(page)
  await expect(dialog.locator('.modal-heading [role=tab]')).toHaveText(['任务', 'DAG', '预设'])
  await dialog.getByRole('textbox', { name: '任务名称', exact: true }).fill('Synthetic task draft')
  await dialog.getByRole('textbox', { name: '指令', exact: true }).fill('Preserve this instruction')
  await expect(dialog.locator('.creation-section-tabs [role=tab]')).toHaveText([
    '任务内容',
    '调度设置',
  ])
  await dialog.getByRole('tab', { name: '调度设置', exact: true }).click()
  const scheduleMetrics = () =>
    dialog.locator('.creation-schedule-scroll:visible').evaluate((element) => {
      const label = element.querySelector('.field > label')!
      const control = element.querySelector('.choice-selector')!
      const field = element.querySelector('.field')!
      return {
        labelFont: getComputedStyle(label).fontSize,
        controlFont: getComputedStyle(control).fontSize,
        padding: getComputedStyle(field).padding,
        height: field.getBoundingClientRect().height,
      }
    })
  const taskScheduleMetrics = await scheduleMetrics()
  expect(taskScheduleMetrics.labelFont).toBe('12px')
  expect(taskScheduleMetrics.controlFont).toBe('12px')
  await page.screenshot({ path: test.info().outputPath('creation-task-wide.png') })
  await page.setViewportSize({ width: 390, height: 844 })
  expect(await dialog.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true)
  await page.screenshot({ path: test.info().outputPath('creation-task-narrow.png') })
  await page.setViewportSize({ width: 1440, height: 900 })
  await dialog.getByRole('tab', { name: 'DAG', exact: true }).click()
  await dialog.getByRole('textbox', { name: '任务名称', exact: true }).fill('Synthetic DAG draft')
  await expect(dialog.locator('.dag-editor')).toBeVisible()
  await expect(dialog.locator('.creation-dag-form .creation-section-tabs [role=tab]')).toHaveText([
    '节点结构',
    '调度设置',
  ])
  await dialog.getByRole('tab', { name: '调度设置', exact: true }).click()
  expect(await scheduleMetrics()).toEqual(taskScheduleMetrics)
  await page.screenshot({ path: test.info().outputPath('creation-dag-schedule-wide.png') })

  await dialog.getByRole('tab', { name: '任务', exact: true }).click()
  await expect(dialog.getByRole('textbox', { name: '任务名称', exact: true })).toHaveValue(
    'Synthetic task draft',
  )
  await expect(dialog.getByRole('tab', { name: '调度设置', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await dialog.getByRole('tab', { name: '任务内容', exact: true }).click()
  await expect(dialog.getByRole('textbox', { name: '指令', exact: true })).toHaveValue(
    'Preserve this instruction',
  )
  await dialog.getByRole('tab', { name: 'DAG', exact: true }).click()
  await expect(dialog.getByRole('textbox', { name: '任务名称', exact: true })).toHaveValue(
    'Synthetic DAG draft',
  )
  await dialog.getByRole('button', { name: '取消', exact: true }).click()
  const confirmation = page.getByRole('dialog', { name: '有未保存的更改', exact: true })
  await expect(confirmation).toBeVisible()
  await confirmation.getByRole('button', { name: '继续编辑', exact: true }).click()
  await expect(dialog.getByRole('textbox', { name: '任务名称', exact: true })).toHaveValue(
    'Synthetic DAG draft',
  )
  await dialog.getByRole('button', { name: '取消', exact: true }).click()
  await discard(page)
  await expect(page.getByRole('dialog')).toHaveCount(0)
})

test('required parameters are editable, numeric drafts and defaults survive tab/preset switches and create preserves origin', async ({
  page,
}) => {
  await loginSession(page, stack.username, password)
  const def = await seed(page, ordinaryDefinition('Required parameters'))
  const other = await seed(page, dagDefinition('Other preset'))
  const dialog = await openCreation(page, '预设')
  await choosePreset(page, dialog, def.name)
  const session = dialog.locator('.preset-session:visible')
  const input = session.getByLabel('仓库地址', { exact: true })
  await expect(input).toBeVisible()
  await expect(session.locator('.template-issues, .template-field-error')).toHaveCount(0)
  await expect(page.getByRole('alert')).toHaveCount(0)
  await session.getByRole('button', { name: '创建', exact: true }).click()
  await expect(page.getByRole('alert')).toContainText('仓库地址')
  await expect(input).toBeFocused()
  await input.fill('https://example.com/first')
  await session.getByLabel('比例', { exact: true }).fill('')
  await session.getByLabel('比例', { exact: true }).pressSequentially('1.')
  await expect(session.getByLabel('比例', { exact: true })).toHaveValue('1.')
  await session.getByLabel('比例', { exact: true }).pressSequentially('25')
  await expect(session.getByLabel('比例', { exact: true })).toHaveValue('1.25')
  await session.getByLabel('比例', { exact: true }).fill('1.25')
  await expect(session.getByRole('button', { name: '创建', exact: true })).toBeEnabled()
  await dialog.getByRole('tab', { name: '任务', exact: true }).click()
  await dialog.getByRole('tab', { name: '预设', exact: true }).click()
  await expect(input).toHaveValue('https://example.com/first')
  await choosePreset(page, dialog, other.name)
  await choosePreset(page, dialog, def.name)
  await expect(input).toHaveValue('https://example.com/first')
  await expect(session.getByLabel('比例', { exact: true })).toHaveValue('1.25')
  const response = page.waitForResponse((response) =>
    response.url().endsWith(`/task-templates/${def.id}/instantiate`),
  )
  await session.getByRole('button', { name: '创建', exact: true }).click()
  const result = await (await response).json()
  await expect(dialog).toHaveCount(0)
  const task = await (await page.request.get(apiPath(`/tasks/${result.task_id}`))).json()
  expect(task.template_origin.template_id).toBe(def.id)
  expect(task.template_origin.resolved_inputs).toMatchObject({
    repository: 'https://example.com/first',
    ratio: 1.25,
    flag: false,
    nullable: null,
  })
  expect(task.config.schedule).toBeNull()
})

test('list edits generate a read-only DAG in the Preset tab, with directory browsing and create-and-run', async ({
  page,
}) => {
  await loginSession(page, stack.username, password)
  const def = await seed(page, dagDefinition('Dynamic repository preset'))
  const dialog = await openCreation(page, '预设')
  await choosePreset(page, dialog, def.name)
  const session = dialog.locator('.preset-session:visible')
  await expect(session.locator('.template-preview-context')).toContainText('3 个节点')
  await session.getByRole('button', { name: '全屏', exact: true }).click()
  await expect(session.locator('.dag-structure-fullscreen')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(session.locator('.dag-structure-fullscreen')).toHaveCount(0)
  await expect(dialog).toBeVisible()
  await expect(
    session.getByRole('combobox', { name: '浏览目录使用的设备', exact: true }),
  ).toHaveCount(0)
  await session.getByRole('button', { name: '选择目录', exact: true }).click()
  const directory = page.getByRole('dialog', { name: '选择工作目录', exact: true })
  await expect(directory).toBeVisible()
  await directory.getByRole('button', { name: '选择文件夹', exact: true }).click()
  await session.locator('.template-array-row').nth(1).getByRole('switch').uncheck()
  await expect(session.locator('.template-preview-context')).toContainText('2 个节点')
  await expect(dialog.getByRole('tab', { name: '预设', exact: true })).toHaveAttribute(
    'aria-selected',
    'true',
  )
  await expect(session.locator('.dag-canvas-mount')).toHaveCount(0)
  await session.getByRole('button', { name: '第 1 项的操作', exact: true }).click()
  await page.getByRole('menuitem', { name: '下移', exact: true }).click()
  await expect(
    session.locator('.template-array-row').first().getByLabel('key', { exact: true }),
  ).toHaveValue('beta')
  const response = page.waitForResponse((response) =>
    response.url().endsWith(`/task-templates/${def.id}/instantiate`),
  )
  await session.getByRole('button', { name: '创建并运行', exact: true }).click()
  const result = await (await response).json()
  expect(result.run_id).toBeTruthy()
  const task = await (await page.request.get(apiPath(`/tasks/${result.task_id}`))).json()
  expect(task.config.dag.nodes.map((node: any) => node.id)).toEqual(['review__alpha', 'summary'])
  expect(task.config.dag.nodes[1].pre).toEqual(['review__alpha'])
})

test('manual Task and DAG creation keep their own editors and save through the unified modal', async ({
  page,
}) => {
  await loginSession(page, stack.username, password)
  let dialog = await openCreation(page)
  await dialog
    .getByRole('textbox', { name: '任务名称', exact: true })
    .fill('Synthetic manual task ' + randomUUID().slice(0, 8))
  await dialog.getByRole('textbox', { name: '指令', exact: true }).fill('{"text":"manual task"}')
  await selectChoice(page, dialog, 'Runner', stack.runner_name)
  await selectChoice(page, dialog, 'Agent', 'Mock Events')
  await dialog.getByRole('button', { name: '选择目录', exact: true }).click()
  await page
    .getByRole('dialog', { name: '选择工作目录', exact: true })
    .getByRole('button', { name: '选择文件夹', exact: true })
    .click()
  await dialog.getByRole('tab', { name: '调度设置', exact: true }).click()
  await selectChoice(page, dialog, '类型', 'manual')
  await dialog.getByRole('button', { name: '创建', exact: true }).click()
  await expect(dialog).toHaveCount(0)
  dialog = await openCreation(page, 'DAG')
  await dialog
    .getByRole('textbox', { name: '任务名称', exact: true })
    .fill('Synthetic manual DAG ' + randomUUID().slice(0, 8))
  await dialog
    .getByRole('button', { name: `${stack.runner_name} 上的 Mock Events`, exact: true })
    .click()
  await dialog.getByRole('textbox', { name: '指令', exact: true }).fill('{"text":"manual DAG"}')
  await dialog.getByRole('button', { name: '选择目录', exact: true }).click()
  await page
    .getByRole('dialog', { name: '选择工作目录', exact: true })
    .getByRole('button', { name: '选择文件夹', exact: true })
    .click()
  await dialog.getByRole('tab', { name: '调度设置', exact: true }).click()
  await selectChoice(page, dialog, '类型', 'manual')
  await dialog.getByRole('button', { name: '创建', exact: true }).click()
  await expect(dialog).toHaveCount(0)
})

test('preset catalog is read-only, invalid links show errors, and use opens accessible bilingual responsive creation', async ({
  page,
}) => {
  await loginSession(page, stack.username, password)
  const def = await seed(page, dagDefinition('Read-only preset'))
  await page.goto(`/w/${workspace}/presets?template=${def.id}`)
  await expect(page.locator('.preset-overview')).toContainText(def.description)
  const overview = page.locator('.preset-overview')
  await expect(overview.locator('.task-overview-primary')).toContainText('仓库')
  await expect(overview.locator('.task-overview-context dt')).toContainText(['设备', 'Agent'])
  await expect(overview.locator('.task-overview-context')).not.toContainText('·')
  await expect(page.locator('.preset-source')).toHaveCount(0)
  await expect(page.locator('.preset-row .row-meta')).toHaveCount(0)
  await page.screenshot({ path: test.info().outputPath('preset-overview-wide.png') })
  await page.setViewportSize({ width: 390, height: 844 })
  await expect(overview).toHaveAttribute('data-size', 'small')
  expect(await overview.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(
    true,
  )
  await page.screenshot({ path: test.info().outputPath('preset-overview-narrow.png') })
  await page.setViewportSize({ width: 1440, height: 900 })
  await expect(page.getByRole('button', { name: '新建预设', exact: true })).toHaveCount(0)
  await expect(page.getByRole('button', { name: '预设操作', exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '使用预设', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '新建任务', exact: true })
  await expect(dialog.locator('.template-preview-context')).toContainText('3 个节点')
  await expect(dialog.locator('.dag-graph-surface')).toBeVisible()
  for (const [width, theme] of [
    [1440, 'light'],
    [1024, 'dark'],
    [390, 'light'],
    [390, 'dark'],
  ] as const) {
    await page.setViewportSize({ width, height: 900 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(dialog).toBeVisible()
    expect(await dialog.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(
      true,
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    if (width === 390) {
      await dialog.getByRole('button', { name: '任务预览', exact: true }).click()
      await expect(dialog.locator('.template-preview-pane')).toBeVisible()
      await expect(dialog.locator('.template-parameters-pane')).toBeHidden()
      await dialog.getByRole('button', { name: '参数', exact: true }).click()
    }
    await page.screenshot({ path: test.info().outputPath(`creation-${width}-${theme}.png`) })
  }
  await setLanguage(page, 'en')
  await expect(page.locator('.creation-modal .modal-heading [role=tab]')).toHaveText([
    'Task',
    'DAG',
    'Preset',
  ])
  await expect(
    page.locator('.creation-modal').getByRole('button', { name: 'Create and run', exact: true }),
  ).toBeEnabled()
  await setLanguage(page, 'zh')
  await dialog.getByRole('button', { name: '取消', exact: true }).click()
  await page.goto(`/w/${workspace}/presets?template=missing-preset`)
  await expect(page.getByRole('alert')).toBeVisible()
  await expect(page.locator('.preset-overview')).toHaveCount(0)
})

test('parameter updates keep the graph view and model input; a new preset revision requires explicit loading', async ({
  page,
}) => {
  await loginSession(page, stack.username, password)
  const definition = dagDefinition('Stable preview')
  definition.bindings.worker.defaults.agent = 'mock-sync'
  const def = await seed(page, definition)
  const dialog = await openCreation(page, '预设')
  await choosePreset(page, dialog, def.name)
  const session = dialog.locator('.preset-session:visible')
  const create = session.getByRole('button', { name: '创建', exact: true })
  await expect(create).toBeEnabled()
  await session.getByRole('button', { name: '表格', exact: true }).click()
  const search = session.getByRole('textbox', { name: '搜索节点', exact: true })
  await search.fill('alpha')
  const graph = await session.locator('.dag-structure').elementHandle()
  await session.locator('.template-binding > summary').click()
  await selectChoice(page, session, '模型', '自定义模型 ID')
  const model = session.getByRole('textbox', { name: '模型', exact: true })
  await model.pressSequentially('custom-model', { delay: 80 })
  await expect(model).toBeFocused()
  await expect(model).toHaveValue('custom-model')
  await expect(create).toBeEnabled()
  await session.getByLabel('备注', { exact: true }).fill('keep graph and search')
  await expect(create).toBeEnabled()
  expect(await graph!.evaluate((element) => element.isConnected)).toBe(true)
  await expect(search).toHaveValue('alpha')
  await expect(session.locator('.dag-node-table')).toBeVisible()
  await session.getByLabel('备注', { exact: true }).fill('unfinished draft')
  await page.request.put(apiPath(`/task-templates/${def.id}`), {
    data: {
      definition: { ...def, description: 'Updated synthetic description' },
      expected_revision: 1,
    },
    headers: { 'x-wakeit-request': '1' },
  })
  await page.evaluate(() => window.dispatchEvent(new Event('focus')))
  await expect(dialog.getByRole('button', { name: '载入新版', exact: true })).toBeVisible({
    timeout: 15000,
  })
  await expect(create).toBeDisabled()
  await expect(session.getByLabel('备注', { exact: true })).toHaveValue('unfinished draft')
  await dialog.getByRole('button', { name: '载入新版', exact: true }).click()
  await expect(dialog.locator('.preset-choice-heading')).toContainText(
    'Updated synthetic description',
  )
  await expect(session.getByLabel('备注', { exact: true })).toHaveValue('unfinished draft')
  await expect(create).toBeEnabled()
})

test('conditional environments remain editable with an old preview, and missing/nullable values can be set explicitly', async ({
  page,
}) => {
  await loginSession(page, stack.username, password)
  const def = dagDefinition('Conditional environment') as any
  def.inputs.properties.summarize.default = false
  def.inputs.properties.empty_list = { type: 'array', title: '空列表', items: { type: 'string' } }
  def.inputs.properties.empty_flag = { type: 'boolean', title: '空布尔值' }
  def.inputs.properties.optional = { type: ['string', 'null'], title: '可空内容', default: null }
  def.inputs.properties.optional_rows = {
    type: 'array',
    title: '可空对象列表',
    default: [null],
    items: {
      type: ['object', 'null'],
      title: '可空行',
      additionalProperties: false,
      properties: { note: { type: 'string', title: '行备注' } },
    },
  }
  def.bindings.summary = { title: '汇总环境', defaults: {} }
  def.task.dag.nodes[1].binding = 'summary'
  await seed(page, def)
  const dialog = await openCreation(page, '预设')
  await choosePreset(page, dialog, def.name)
  const session = dialog.locator('.preset-session:visible')
  const create = session.getByRole('button', { name: '创建', exact: true })
  await expect(create).toBeEnabled()
  const nullable = session.locator('[data-input-path="inputs.optional"]')
  await nullable.getByRole('button', { name: '设置值', exact: true }).click()
  await nullable.getByRole('textbox').fill('synthetic value')
  await nullable.getByRole('button', { name: '可空内容 的参数操作', exact: true }).click()
  await page.getByRole('menuitem', { name: '设为空值', exact: true }).click()
  await expect(nullable).toContainText('空值')
  const nullableRow = session.locator('[data-input-path="inputs.optional_rows[0]"]').first()
  await expect(nullableRow).toContainText('空值')
  await nullableRow.getByRole('button', { name: '设置值', exact: true }).click()
  await nullableRow.getByRole('textbox', { name: '行备注', exact: true }).fill('optional value')
  await nullableRow.getByRole('button', { name: '行备注 的参数操作', exact: true }).click()
  await page.getByRole('menuitem', { name: '清除', exact: true }).click()
  await expect(nullableRow.getByRole('textbox', { name: '行备注', exact: true })).toHaveValue('')
  await session.getByRole('button', { name: '空列表 的参数操作', exact: true }).click()
  await page.getByRole('menuitem', { name: '设为空列表', exact: true }).click()
  await session.getByRole('button', { name: '空布尔值 的参数操作', exact: true }).click()
  await page.getByRole('menuitem', { name: '设为否', exact: true }).click()
  await session.getByRole('switch', { name: 'summarize', exact: true }).check()
  const binding = session.locator('[data-input-path="bindings.summary"]')
  await expect(binding).toBeVisible()
  await create.click()
  await expect(page.getByRole('alert')).toBeVisible()
  await selectChoice(page, binding, '汇总环境 的设备', stack.runner_name)
  await selectChoice(page, binding, '汇总环境 的 Agent', 'Mock Events')
  await expect(create).toBeEnabled()
  const response = page.waitForResponse((response) =>
    response.url().endsWith(`/task-templates/${def.id}/instantiate`),
  )
  await create.click()
  const result = await (await response).json()
  const task = await (await page.request.get(apiPath(`/tasks/${result.task_id}`))).json()
  expect(task.template_origin.resolved_inputs).toMatchObject({
    empty_list: [],
    empty_flag: false,
    optional: null,
    optional_rows: [{}],
  })
})

test('a failed initial preview still allows correcting model parameters without losing input focus', async ({
  page,
}) => {
  await loginSession(page, stack.username, password)
  const def = ordinaryDefinition('Invalid initial model') as any
  def.inputs.properties.repository.default = 'https://example.com/synthetic'
  def.bindings.worker.defaults.agent = 'mock-sync'
  def.bindings.worker.defaults.parameters.model = 'x'.repeat(4097)
  await seed(page, def)
  const dialog = await openCreation(page, '预设')
  await choosePreset(page, dialog, def.name)
  const session = dialog.locator('.preset-session:visible')
  const create = session.getByRole('button', { name: '创建', exact: true })
  await expect(session.locator('.template-issues, .template-field-error')).toHaveCount(0)
  await expect(create).toBeEnabled()
  await create.click()
  await expect(page.getByRole('alert')).toBeVisible()
  const model = session.getByRole('textbox', { name: '模型', exact: true })
  await model.fill('')
  await model.pressSequentially('fixed-model', { delay: 120 })
  await expect(model).toBeFocused()
  await expect(model).toHaveValue('fixed-model')
  await expect(create).toBeEnabled()
})

test('adding an unfinished repository stays silent; submit reports once and focuses it', async ({
  page,
}) => {
  await loginSession(page, stack.username, password)
  const def = ordinaryDefinition('Repository list') as any
  def.inputs.properties.repository = {
    type: 'array',
    title: '仓库地址',
    minItems: 1,
    description: 'Each entry is a repository address.',
    items: { type: 'string', title: '仓库地址', minLength: 1, format: 'repository-url' },
    default: ['https://github.com/example/project'],
  }
  def.task.name = 'Repository review'
  def.task.prompt = { template: 'Review [[ inputs.repository | join(", ") ]]' }
  await seed(page, def)
  const dialog = await openCreation(page, '预设')
  await choosePreset(page, dialog, def.name)
  const session = dialog.locator('.preset-session:visible')
  const ready = session.locator('.template-ready')
  await expect(ready).toHaveAttribute('aria-label', '可以创建')
  await expect(ready).toHaveCSS('color', 'rgb(31, 136, 61)')
  await ready.focus()
  await expect(page.getByRole('tooltip')).toHaveText('可以创建')
  await expect(dialog.locator('.preset-choice-title .icon-button')).toBeVisible()
  const title = await dialog.locator('.preset-choice-title > strong').boundingBox()
  const info = await dialog.locator('.preset-choice-title .icon-button').boundingBox()
  expect(info!.x - (title!.x + title!.width)).toBeLessThan(12)
  await expect(dialog.getByRole('button', { name: '切换预设', exact: true })).toHaveCSS(
    'border-top-width',
    '1px',
  )
  const bindingHeader = session.locator('.template-binding > summary').first()
  await expect(bindingHeader.locator('svg')).toHaveCount(2)
  await expect(bindingHeader.locator('.agent-logo')).toBeVisible()
  await expect(session.locator('.template-bindings > h3')).toHaveCount(0)
  await setLanguage(page, 'en')
  await expect(page.locator('.creation-modal .template-ready')).toHaveAttribute(
    'aria-label',
    'Ready to create',
  )
  await expect(
    page.locator('.creation-modal').getByRole('button', { name: 'Change preset', exact: true }),
  ).toBeVisible()
  await setLanguage(page, 'zh')
  const list = session.locator('[data-input-path="inputs.repository"]').first()
  const response = page.waitForResponse((response) =>
    response.url().endsWith(`/task-templates/${def.id}/preview`),
  )
  await list.getByRole('button', { name: '添加一项', exact: true }).click()
  await response
  const empty = list.locator('input').nth(1)
  await expect(empty).toHaveValue('')
  await expect(
    session.locator('[aria-invalid=true], .template-field-error, .template-issues'),
  ).toHaveCount(0)
  await expect(page.getByRole('alert')).toHaveCount(0)
  await expect(list.locator('.template-array-footer > :not(button)')).toHaveCount(0)
  await expect(list.locator('.template-array-footer button svg')).toBeVisible()
  await session.getByRole('button', { name: '创建', exact: true }).click()
  await expect(page.getByRole('alert')).toHaveCount(1)
  await expect(page.getByRole('alert')).toContainText('仓库地址')
  await expect(empty).toBeFocused()
  await page.screenshot({ path: test.info().outputPath('repository-submit-toast.png') })
  await empty.fill('https://github.com/example/another')
  const splitter = session.getByRole('separator', { name: '调整参数区宽度' })
  const before = Number(await splitter.getAttribute('aria-valuenow'))
  await splitter.focus()
  await splitter.press('ArrowRight')
  await expect(splitter).toHaveAttribute('aria-valuenow', String(before + 10))
  await page.screenshot({ path: test.info().outputPath('repository-parameters.png') })
})

test('DAG palette drag uses viewport coordinates and canvas supports node movement and panning', async ({
  page,
}) => {
  await loginSession(page, stack.username, password)
  const dialog = await openCreation(page, 'DAG')
  const entry = dialog.getByRole('button', {
    name: `${stack.runner_name} 上的 Mock Events`,
    exact: true,
  })
  const logo = await entry.locator('.agent-logo').boundingBox()
  const canvas = dialog.locator('.react-flow__pane')
  const rect = await canvas.boundingBox()
  expect(logo).toBeTruthy()
  expect(rect).toBeTruthy()
  const x = rect!.x + rect!.width * 0.45,
    y = rect!.y + rect!.height * 0.4
  await page.mouse.move(logo!.x + 6, logo!.y + 6)
  await page.mouse.down()
  await page.mouse.move(x, y, { steps: 12 })
  const ghost = page.locator('body > .dag-agent-ghost')
  await expect(ghost).toBeVisible()
  const shadow = await ghost.boundingBox()
  expect(Math.abs(shadow!.x + shadow!.width / 2 - x)).toBeLessThan(3)
  expect(Math.abs(shadow!.y + shadow!.height / 2 - y)).toBeLessThan(3)
  await page.mouse.up()
  await expect(ghost).toHaveCount(0)
  const node = dialog.locator('.react-flow__node').first()
  await expect(node).toBeVisible()
  await dialog
    .locator('.dag-floating-panel')
    .getByRole('button', { name: '关闭', exact: true })
    .click()
  const position = await node.boundingBox()
  await page.mouse.move(position!.x + 70, position!.y + 48)
  await page.mouse.down()
  await page.mouse.move(position!.x + 170, position!.y + 108, { steps: 12 })
  const moved = await node.boundingBox()
  expect(moved!.x - position!.x).toBeGreaterThan(60)
  // A real Runner refresh must not reset a node still being dragged.
  await page.waitForResponse((response) => response.url().endsWith('/runners'))
  const refreshed = await node.boundingBox()
  expect(Math.abs(refreshed!.x - moved!.x)).toBeLessThan(3)
  await page.mouse.up()
  const viewport = dialog.locator('.react-flow__viewport')
  const transform = await viewport.getAttribute('style')
  await page.mouse.move(rect!.x + rect!.width - 30, rect!.y + rect!.height - 40)
  await page.mouse.down()
  await page.mouse.move(rect!.x + rect!.width - 110, rect!.y + rect!.height - 100, { steps: 10 })
  await page.mouse.up()
  await expect(viewport).not.toHaveAttribute('style', transform!)
  await page.screenshot({ path: test.info().outputPath('dag-creation-drag.png') })
  await dialog.getByRole('tab', { name: '调度设置', exact: true }).click()
  await expect(dialog.locator('.dag-stepbar')).not.toContainText('Cron')
  await page.screenshot({ path: test.info().outputPath('dag-creation-schedule.png') })
})
