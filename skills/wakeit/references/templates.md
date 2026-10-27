# Task Templates 与创建时生成 DAG

用户需要可复用任务配方、类型化业务输入、按仓库等列表生成多个节点，或把现有 Task 存成模板时使用本指南。任务模板不同于 `agent init` 导出的 Agent 适配模板，也不同于运行时修改 DAG。

## 先确认环境与目标

读取当前 Gateway、workspace 和可用 Runner/Agent：`wakeit --json runner list`、`wakeit --json agent list`。确认实际 Runner 名称或 ID、适配 ID、声明参数和 `allowed_roots`。目录是 Runner 设备上的路径，不是 Gateway 或当前 CLI 所在机器的路径。

确认用户要离线校验、保存模板、只读预览、创建 Task，还是创建并立即执行。预览和校验不创建 Task/Run，也不执行 Agent；只有用户明确要求创建或执行时才调用相应写接口。不要把创建模板等同于立即运行。

WebUI 中，主导航“预设”提供第二侧栏模板列表和右侧“概览 / 预览 / 定义”工作区。普通 Task 或 DAG task 的现有创建页提供“模板（可选）”，默认不使用模板；选择后填写参数、环境并检查生成预览，再使用原有“保存任务”。不要寻找已移除的独立“从模板创建”入口。“保存为模板”会进入预设定义编辑页。

模板格式和配置/消息协议均为 version 1。使用支持这些命令的 CLI/Gateway；旧版本不认识命令时先升级，不要自行拼接运行时扩展。导出当前定义 Schema：`wakeit schema template`。

## 定义和校验

从本 skill 的 `assets/task-template.yaml` 复制普通动态 DAG 示例，替换业务输入、指令与实际绑定。模板包含 `inputs`、`bindings` 和 `task`：

- `inputs` 声明封闭的类型化参数。保留 boolean、数字、数组、对象和允许的 null；不要把列表或对象塞进字符串型 `TaskConfig.parameters`。
- `bindings` 是命名环境槽位，提供 Runner、Agent 和适配参数默认值。业务值与设备凭据分开；不要复制 Agent 凭据或已有 Session ID。
- `task` 是普通 Task 或完整 DAG 蓝图。只有显式 `{template: ...}` 文本使用 MiniJinja，普通字符串始终为字面量。文本插值为 `[[ ... ]]`，结构表达式为 `{expr: "..."}`。

```sh
wakeit template validate --file task-template.yaml
wakeit template apply --file task-template.yaml
wakeit --json template list
wakeit template get <template-id>
```

`validate` 离线检查定义和语法，不要求 Gateway 登录。`apply` 才保存到当前 workspace；更新已读取模板时加 `--expect <revision>`，冲突后读取最新定义并保留双方意图，不盲目覆盖。`archive` / `restore` 只影响新实例，不改变已有 Task 或 Run。

## 输入、绑定和预览

覆盖文件是单个 JSON 兼容 YAML/JSON 文档：

```yaml
values:
  working_directory: /absolute/runner/path
  repositories:
    - {key: wakeit, url: 'https://github.com/mob999/wakeit', enabled: true}
  include_summary: true
unset: [optional_note]
```

未覆盖的顶层字段使用默认值；`values` 整体替换数组/对象，不深合并；`unset` 抑制默认值并省略字段。`false`、`0`、空字符串、空数组/对象及允许的 null 都是实际值，不等于省略。未知字段、重复 key、错误类型、同时提供和 unset 都必须先修正。

绑定文件按槽位提供实际环境：

```yaml
worker:
  runner: dev-server
  agent: codex
  parameters: {}
```

```sh
wakeit template preview <template-id> --inputs-file values.yaml --bindings-file bindings.yaml
```

检查返回的 `issues`、最终 `config`/`graph`、`resolved_inputs`、环境槽位和 source map。预览可以显示带错误的生成图，但 `config` 为空时不能创建。确认节点数量、实际 ID、依赖、输出、工作目录和指令均符合用户意图；不要凭空补充缺失 Runner、Agent 或目录。预览不会创建 Task、Run、事件或 Outbox，也不会伪造上游结果。

## 动态 DAG 约束

固定节点使用字面 `id`。展开组使用 `group`、`for_each`、`as`、`key` 和 `node`；`for_each` 必须返回有序对象数组。每项的 key 属性是非空、唯一且稳定的字符串，生成 ID 为 `<group>__<key>`。重排列表保留其他 key 的身份，修改 key 会改变身份；不要依赖数组下标或让服务截断冲突 ID。

`when` 可筛选节点、列表项、依赖和输出。使用 `{node: ...}`、`{group: ..., key: ...}` 或 `{group: ..., all: true}` 选择实际节点。指向被排除或不存在的目标会失败；空组的 `all` 可为空，但最终图必须至少有一个节点，显式 `dag.results` 必须非空且唯一。筛选掉的节点不是运行时 skipped 节点。

DAG 指令由独立 text/template/result parts 组成。模板只在创建时渲染一次；保存后的 text 不再解释 `[[ ... ]]`、`{% ... %}` 或 `{{ nodes.x.result }}`，result 才读取同一 Run 的具体上游输出。不要通过拼接字符串伪造依赖，或把旧 DAG 任务直接注册成模板任务。

## 创建、执行与转换

```sh
wakeit --request-key <stable-operation-key> --json task create \
  --template <template-id> --inputs-file values.yaml --bindings-file bindings.yaml
wakeit --request-key <another-operation-key> --json task create \
  --template <template-id> --inputs-file values.yaml --bindings-file bindings.yaml --run
```

可加 `--schedule-file`、`--id`、`--name` 和 `--expect`。`--run` 原子创建一个独立 Task 和一次 Run；不加时只创建 Task。返回实际 `task_id`、`task_revision`、`run_id` 和 `template_revision`，不要用本地猜测的 ID 替代。

同一网络请求失败后的传输重试复用原 `--request-key` 和完整请求；用户明确发起的另一个实例使用新键。相同已提交操作返回原 ID，即使模板之后已修改或归档；同键不同请求、过期 revision 或 Task ID 冲突返回 409。校验失败不会产生半创建的 Task、Run 或 Outbox。

创建并执行重复调度时，下一次计划在当前时间之后；已到期的一次性计划与立即执行合并，未来的一次性计划则保留立即执行和未来执行。创建后 Task 是调度与后续整图重跑的权威配置，模板编辑不会实时继承，历史 Run 与节点恢复继续使用冻结图。

将现有 Task 转为模板：`wakeit template from-task <task-id> [--file <new-path>]`。它保留字面文本和具体结果引用，不推断业务输入，不复制执行历史或 Session 绑定，也不覆盖已有文件。需要参数化时由用户明确修改新模板。

## 升级与兼容

部署模板功能前先备份并确认没有活动 Run，停止所有旧 Gateway 写入，执行追加迁移，再统一启动支持结构化 Prompt 的新 Gateway。不要让旧 Gateway 调度或恢复这些 Task，不要新旧混写。Runner 仍接收普通 version-1 执行指令，保留设备身份、原生会话和 Agent 凭据，无需重新注册。
