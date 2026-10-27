# 任务模板与生成 DAG

任务模板是工作区内可复用的任务配方。它声明业务参数、执行环境绑定和普通任务或 DAG 蓝图。使用模板时，Gateway 在**创建前**解析参数、生成完整任务配置并校验；确认后才创建独立 Task，或原子创建 Task 和一次 Run。

模板不会在每次调度或执行时重新渲染。修改、归档模板不会改变已有 Task 或历史 Run；编辑已生成的 Task 也不会回写模板。已有 Task 可以另存为新模板，但不会自动推断参数或恢复曾经的生成器。

## WebUI

任务列表的「＋」打开统一创建窗口，顶部依次为「任务 / DAG / 预设」。前两页用于手动编写普通任务或 DAG；「预设」是独立的实例化页面，生成 DAG 时仍留在该页，不进入 DAG 编辑器。

主导航「预设」提供只读的分页列表与详情，通过全局搜索查找。详情展示用途、参数与默认值、执行环境及可选的只读任务定义。点击「使用预设」直接进入创建窗口并选中该预设。WebUI 不提供预设定义编辑、保存、归档、恢复或从任务保存为预设的操作；这些管理能力使用下方的 CLI/API。

使用预设创建任务：

1. 搜索并选择预设。参数与预览分别滚动；窄窗口通过「参数 / 任务预览」切换，创建操作固定在底部。
2. 直接填写业务参数。必填项立即提供控件，默认值可直接编辑；字段菜单可恢复默认、清空、显式省略默认或设置可空值。`false`、`0`、空字符串、空列表与 `null` 保留各自含义。
3. 列表对象使用紧凑表格，支持添加、复制、调整顺序和删除；复杂属性放在行详情。目录浏览使用对应设备，多环境时明确选择浏览设备。模型查询使用实际生成的工作目录，多个目录时显式选择查询上下文。
4. 检查只读的任务预览及调度摘要。普通任务展示最终指令与执行环境；DAG 展示限高的图/表及所选节点指令。输入变化保留已有预览和图表阅读状态，并使其暂时不可提交；错误显示在字段旁，点击错误可定位对应字段。执行环境以可展开摘要放在参数上方，缺失或错误时自动展开。
5. 校验通过后选择「创建」或「创建并运行」。创建沿用预设的名称、调度和执行配置，生成的任务独立保存来源快照，后续可按普通任务编辑。

本次窗口内切换创建方式或预设会保留各自草稿。填写期间固定预设版本，发现更新后提示「载入新版」，由用户主动载入并重新校验兼容参数。创建前不会保存任务或执行；只有最新输入对应的预览校验通过才能提交。普通任务和 DAG 创建均可直接点击「创建」，默认调度始终显示在摘要中。

## CLI

```sh
wakeit template validate --file examples/task-template.yaml
wakeit template apply --file examples/task-template.yaml
wakeit --json template list
wakeit template get repository-monitor
wakeit template preview repository-monitor --inputs-file values.yaml --bindings-file bindings.yaml
wakeit --request-key create-review-001 --json task create --template repository-monitor \
  --inputs-file values.yaml --bindings-file bindings.yaml
wakeit --request-key create-review-002 --json task create --template repository-monitor \
  --inputs-file values.yaml --bindings-file bindings.yaml --run
```

`template validate` 在本机校验定义及模板语法，不连接 Gateway，也不创建任务。`apply` 保存完整定义；更新已读取版本时使用 `--expect <revision>` 防止覆盖他人的修改。`archive` / `restore` 改变模板能否用于新实例，不影响已有 Task。`template from-task <task-id>` 输出转换后的定义，`--file <path>` 保存到新文件且不覆盖已有文件。

参数覆盖文件使用 JSON 兼容的 YAML 或 JSON：

```yaml
values:
  working_directory: /srv/reviews
  repositories:
    - key: wakeit
      url: https://github.com/mob999/wakeit
      enabled: true
    - key: docs
      url: git@example.com:team/docs.git
      enabled: false
  include_summary: true
unset: [note]
```

省略的顶层参数使用声明的默认值；`values` 中的值整体替换默认值，数组和对象不做深合并；`unset` 明确省略该值并抑制默认值。`false`、`0`、`""`、`[]`、`{}` 和允许的 `null` 都是实际值，不等于省略。同一字段不能同时出现在 `values` 和 `unset`。未提供值的必填字段、未知字段和错误类型都会阻止创建。

环境绑定文件以模板中的槽位名为键：

```yaml
worker:
  runner: dev-server
  agent: codex
  parameters:
    model: ''
```

调度覆盖可通过 `--schedule-file schedule.yaml` 提供，覆盖值是 Task 的 Schedule 对象或 `null`。`--id` 指定新 Task ID，`--name` 使用字面任务名，`--expect` 指定读取到的模板版本。CLI 使用服务端编译器，不信任客户端拼好的最终图。

创建和创建并执行要求幂等键。CLI 为一次提交生成操作键；网络传输失败后的同一次重试必须复用原请求和键，明确的新实例使用新键。相同已提交操作返回原 Task/Run ID，即使模板随后已编辑或归档；同键不同请求、版本冲突或 Task ID 冲突返回 409。

创建并执行重复调度时，下一次计划严格晚于当前时间。已到期的一次性计划与立即执行合并，不会产生第二次立即执行；未来的一次性计划则保留一次立即执行和所选时间的后续执行。

## 定义与渲染

完整可解析示例见 [examples/task-template.yaml](../examples/task-template.yaml)，公开 Schema 见 [schemas/task-template.v1.json](../schemas/task-template.v1.json)，也可运行 `wakeit schema template`。

模板格式 `version: 1` 包含：

- `inputs`：封闭的 JSON 兼容参数定义，支持 string、boolean、integer、number、array、object 和 nullable 类型，以及必填、枚举、默认值、长度和数量范围。目录与仓库地址使用独立 format；不会探测仓库或执行命令。
- `bindings`：稳定的执行环境槽位，可提供 Runner、Agent 和适配参数默认值。业务参数不写入现有字符串型 `TaskConfig.parameters`。
- `task`：普通任务或 `dag` 蓝图，包含名称、调度、执行策略、固定节点和命名展开组。

只有显式 `{template: "..."}` 的文本会渲染，普通字符串始终为字面量。文本插值使用 `[[ expression ]]`，条件和循环使用 `{% if %}` / `{% for %}`，注释使用 `{# ... #}`。结构表达式使用 `{expr: "..."}`，例如节点条件和列表展开。实现使用严格未定义值的 MiniJinja；拼错的字段不会被静默替换为空字符串。

模板上下文只有已解析的 `inputs`，以及展开组声明的当前项别名。没有文件加载、跨模板继承、环境变量、时钟、网络或 shell 函数。数组/对象需要 JSON 时显式使用 `tojson`。模板渲染有燃料和输出上限；单个生成文本沿用现有 1 MiB 指令边界。

## 创建时生成 DAG

固定节点使用字面 `id`。展开组使用：

```yaml
group: review
for_each: { expr: inputs.repositories }
as: repo
key: key
when: { expr: 'repo.enabled | default(true)' }
node:
  # 每个保留项生成一个节点
```

`for_each` 必须返回有序对象数组，`key` 指定每项中非空、唯一且稳定的字符串属性。节点 ID 为 `<group>__<key>`，例如 `review__wakeit`。列表顺序决定定义顺序；插入、删除和重排保留其他 key 的身份。修改 key 就是修改节点身份，不能用截断或数组下标掩盖冲突。

条件可筛选固定节点、展开项、依赖和输出。依赖、结果引用与输出选择使用类型化选择器：

```yaml
{node: summarize}
{group: review, key: wakeit}
{group: review, key: {expr: repo.key}}
{group: review, all: true}
```

选择器也可带 `when`。条件为 false 时不求值 key、不查找目标、不插入分隔符。指向未生成的固定节点或 key 会报错；声明存在但没有成员的组可用 `all` 展开为空列表。最终图至少要有一个节点，显式输出必须非空、唯一且指向实际节点。省略 `dag.results` 仍选择当前叶节点。

DAG 蓝图的指令由有序 parts 组成：

- `{text: "字面文本"}`
- `{template: "[[ inputs... ]] 与 {% ... %} 块"}`
- `{result: <选择器>, separator: "可选字面分隔符"}`

创建后，文本和已渲染模板都保存为字面文本，结果选择器展开为具体上游节点 ID。只有结果部分会在同一次 Run 中读取实际上游输出；输入文本中的 `[[ ... ]]`、`{% ... %}` 或 `{{ nodes.x.result }}` 不会被再次执行。缺失、过期、不完整或截断的必需上游结果仍按原 DAG 规则失败。

## API 与一致性

所有模板业务接口位于 `/api/v1/workspaces/{workspace-id}`，完整请求/响应模型见 [REST API](api.md) 和 `schemas/openapi.v1.json`。

| 接口 | 行为 | 权限 |
|---|---|---|
| GET `/task-templates` | 分页、搜索、按归档状态列出模板 | Viewer |
| GET `/task-templates/{id}` | 定义和当前 revision | Viewer |
| PUT `/task-templates/{id}` | 保存完整定义，使用 expected_revision 保护更新 | Operator |
| POST `/task-templates/{id}/archive`、`/restore` | 改变新实例可用性 | Operator |
| POST `/task-template-preview` | 预览未保存草稿 | Viewer |
| POST `/task-templates/{id}/preview` | 按当前 revision、输入和绑定生成只读预览 | Viewer |
| POST `/task-templates/{id}/instantiate` | 原子创建 Task，可选同时创建 Run | Operator |
| GET `/tasks/{id}/template` | 把现有 Task 转为新模板定义，保留文本和具体结果引用 | Viewer |

预览返回 revision、已解析输入、最终配置或图、环境使用情况和 source map。校验问题包含稳定的输入/蓝图路径，以及可用的节点、行列信息。预览不会创建 Task、Run、事件或 Outbox 记录，也不会伪造上游输出。

实例化在服务端重新编译并复核 revision、归档状态、成员权限、绑定和目录。校验失败不产生部分写入；Task、来源记录、可选 Run、审计和幂等结果在同一事务中提交。Task/Run 保存模板 ID、名称、revision、原始覆盖、已解析输入、绑定及组/key 到节点的来源关系，作为创建时快照，而不是实时继承。

所有 Gateway 升级并理解结构化 Prompt 之前，不应让旧 Gateway 调度或恢复包含它的 Task。升级时暂停旧 Gateway 写入，完成迁移后统一启动新版本；Runner 仍接收普通 version-1 执行指令，无需因该格式升级重新注册或更换 Agent 凭据。
