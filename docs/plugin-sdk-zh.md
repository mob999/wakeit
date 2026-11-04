# 插件 SDK

[English](plugin-sdk.md)

本文描述当前源码中的 SDK 契约，不表示所有已发布版本均已具备这些能力。对接已安装的 CLI 时，以该版本提供的 schema 和实际支持的操作为准。

统一 SDK 为插件包注册 Agent、事件处理器、生命周期 Hook、动作和查询 Provider，由 Runner 作为独立进程管理，不在 Gateway 进程内运行。普通扩展无需包含 Agent。

安装和来源身份沿用[插件包](plugins-zh.md)。新包通过 `wakeit.plugins` 声明统一入口；可执行 `commands` 和旧 `agents`／`extensions` 输入继续兼容。每个扩展实例在自己的 Runner 上保存配置、凭证和私有数据；代码可以共享，工作区之间不共享实例数据。

## Agent 回调

在插件的 `agents` 映射中声明 Descriptor，格式见[Agent 插件](plugins-zh.md#编写插件)。同步 Agent 声明 `detect` 和 `execute`；异步 Agent 声明 `detect`、`submit` 和 `poll`。仅在实现后声明核对、模型和会话等可选操作；会话操作要求具备检查能力。独立 `cancel` operation 面向原生任务，不用于取消活动中的同步请求。回调必须与声明一致，不能用普通 action 冒充 Agent。

以下 TypeScript 注册方式假定自己的 `adapter.js` 提供与清单一致的 Descriptor 和类型化检测、执行回调：

```ts
import { Agent, Plugin } from '@wakeit/plugin-sdk'
import { descriptor, detect, execute } from './adapter.js'

await new Plugin()
  .agent('example', new Agent(descriptor)
    .on('detect', detect)
    .on('execute', execute))
  .serve()
```

Rust 使用 `wakeit_plugin_sdk::agent::Agent`，通过 `Agent::new(descriptor).on(AgentOperation::Detect, detect).on(AgentOperation::Execute, execute)` 和 `Plugin::new().agent("example", agent)` 注册。两种 SDK 的 Agent 契约均不依赖 Core、CLI 或 Gateway。Rust 通过 `wakeit-plugin-protocol` 共享 wire DTO；插件作者引用 SDK，而非宿主配置类型。Rust task API 接收公开 JSON 配置（`save_task(id, config, revision, request_key)`），不接收 Core `TaskConfig`。

宿主通过 `agent.invoke` 发送类型化 `AgentRequest`，回调返回 `AgentReply`。执行上下文包含已授权的工作目录、期限、参数、Run／Task 身份和任务指令；非执行查询不接收任务指令。会话操作使用精确的原生 Session ID，不回退到“最新会话”，检查时不能创建会话。

执行期间使用 Agent 上下文的 `record`、`trace`、`log`、`session` 和 `result` 输出公开内容。每条 `agent.record` 携带请求／Run 身份及序号，并等待宿主确认；内部仍使用 version-1 `StreamRecord`。不要向 JSON-RPC stdout 直接混写裸事件 JSONL 或原生输出，不把私有数据混入公开记录。

### 原生生命周期与输出收尾

通过 `agent.fact` 报告原生生命周期事实，使用绑定当前 Run 的 `AgentFact {request_id, run_id, seq, fact: NativeFact}`。宿主与 `agent.record` 一样逐条确认；fact 与 record 共用该次调用的**同一条有序序号序列**，不能单独维护 fact 计数器或改变已绑定的请求／Run 身份。

| 原生事实 | 含义 |
|---|---|
| `started {pid}` | 原生进程已实际创建；仅完成准备不算启动。 |
| `input_submitted` | 任务指令已实际交给原生执行，而非仅生成输入。 |
| `native_finished {exit_code}` | 原生进程已结束；该事实本身不代表权威 Run 已完成。 |
| `start_failed {message}` | 原生进程创建失败，附带失败原因。 |

明确没有启动或提交原生工作时，返回 `ExecutionOutcome::NotStarted`，序列化为 `not_started {message}`。已知的 spawn 失败不能误报为 `unknown`／`uncertain`；反过来，未收到回复不等于从未启动，无法核实时仍须保留不确定状态。

真实最终报告可通过 `ctx.result(value)`（已 ACK 的 `StreamRecord::Result`）或 `ExecutionOutcome::Completed.result` 二选一提供，无需重复发送。已有已 ACK 流结果且 `Completed.result` 为 `null` 时，Runner 采用流结果；这是合法的仅流式结果，不是非法占位。当前 ABI 使用 JSON `Value` 字段，并非 `Option`，也没有独立的“未提供”状态，因此不能把 `null` 一概判为非法。

双方都提供非 null 报告时，内容必须相等，包括明确的截断标记；冲突必须失败，不能静默相互覆盖。官方适配器当前在两条通道返回同一份真实报告。生命周期事实不能代替最终报告。

SDK 在成功、回调失败和超时时均关闭该次调用的输出屏障，结束调用前必须处理在途输出的结果；屏障关闭后，迟到或脱离调用的回调不能继续写入。取消后仅允许在屏障仍开放时发送绑定原 Run 的收尾 record／fact，不允许切换到其他 Run、启动新进程、读取凭证或发起其他新的宿主操作。

### 取消控制

活动同步 Run 的取消通过发往**原有 SDK 请求**的 `cancel` 控制帧传递。适配器感知 `AgentContext` 的取消信号并停止原生工作；协作取消未能结束时，Runner 保留强制清理进程的职责。该路径不启动新 worker，也不调用独立的 `AgentOperation::Cancel` 回调。

当前官方 Agent 全部同步执行，均使用上述活动请求控制路径。即使 Descriptor 不声明独立 `cancel` operation，它们仍可取消，`AgentInfo.cancellable` 保持 `true`。

公共 `AgentOperation::Cancel` 用于带 `job_id` 的异步原生任务。只有回调真正请求取消该任务时才声明此能力，并通过 `poll` 确认停止状态；接受取消请求不等于任务已停止。不具备原生任务取消能力的 worker 返回 `unsupported`，不能声明只返回“已接受”、实际不执行取消的空回调。

Runner 负责执行身份、并发、进程管理、记录和权威 Run 状态。原生完成、取消请求和已确认停止是不同结果；无法确认时报告 uncertain，核对时不能盲目重新提交。官方包使用相同 SDK 入口和 `args: ["sdk"]`，不经过官方 YAML 路径。

## 声明扩展

在 `wakeit-plugin.json` 中声明入口、配置 schema、凭证槽、权限和回调 ID。完整示例见[英文指南](plugin-sdk.md#describe-an-extension)。

配置采用 JSON Schema，由设备验证，不下载远程 schema 引用。运行时注册的回调必须与 manifest 一致。声明能力不等于获得授权；管理员授予权限，并将事件和 Hook 绑定到指定实例。

`wakeit schema plugin-sdk` 提供公共数据结构、事件和 Hook 目录。兼容格式的 Agent YAML、任务及输出事件保持 version 1；插件 IPC 独立协商能力。旧宿主不支持新声明时应明确失败，不静默降级。

## 编写扩展

TypeScript SDK 位于 [`sdk/typescript`](../sdk/typescript)。插件作者将 SDK 打包进分发的程序资源；安装时 Wakeit 不执行 npm install 或构建脚本。Node 22.18+ 可运行可擦除类型语法的 TypeScript，也可以分发编译后的 JavaScript。

```ts
import { Plugin, patch } from '@wakeit/plugin-sdk'
const plugin = new Plugin()
plugin.on('completed', async (event, ctx) => {
  await ctx.storage.set('last-event', event)
})
plugin.hook('prepare', async (input, ctx) => {
  const settings = await ctx.config.get<{prefix: string}>()
  return patch({prompt: `${settings.value.prefix}\n${input.prompt}`})
})
plugin.action('inspect', async (_, ctx) => (await ctx.config.get()).value)
await plugin.serve()
```

`completed`、`prepare`、`inspect` 是 manifest 中声明的处理器 ID。通过 `plugin.provider` 注册查询 Provider，通过 `plugin.lifecycleHandler` 注册固定生命周期回调。

Rust 使用 [`wakeit-plugin-sdk`](../crates/wakeit-plugin-sdk)。`Plugin` 注册回调，`Context` 提供调用身份、取消信号和类型化宿主调用。原生二进制沿用平台 artifact 分发。

其它语言可以实现 stdin/stdout 上的逐行 JSON-RPC 2.0。宿主发送 initialize、生命周期、普通扩展 invoke 和类型化 Agent 的 agent.invoke；插件在处理期间可反向调用宿主。请求 ID 关联乱序响应。标准输出只承载协议，诊断使用结构化日志。

## 事件和 Hook

事件表示已提交事实。Gateway 负责 Task、Run、DAG，Runner 负责本机执行。`run.finished` 对应普通运行或 DAG 整轮结算，不是每个子节点 attempt。排队运行也可能直接结束；run.started 表示核心运行进入执行准备窗口，execution.started 才确认原生命令已启动。unknown 不表示完成。DAG 恢复后再次完成会产生新事件。

重试保留事件 ID；显式重放建立新的 delivery 和幂等键。外部副作用可能在进程断连前已经完成，处理器应使用服务提供的幂等能力，并准确报告不确定结果。

完成事件关联当次不可变快照。投递只携带小型元数据和快照引用，读取保存的结果，而非可能已被重试修改的当前 Run。投递期限与正文保留期明确区分。

Hook 在业务操作提交前返回 allow、deny 或 patch，各阶段有明确的可修改字段。执行准备可修改 prompt 和已声明参数，不能替换 Runner、Agent、工作目录、会话或运行身份；变换后重新执行核心验证。

Hook 按配置顺序执行。Gateway 调用设备 Hook 时不持数据库行锁。不可用时按绑定的拒绝、等待或显式跳过策略处理；业务 deny 不自动放行。Hook 上下文只允许读取宿主 API，外部副作用由提交后的事件或独立动作完成。

## 设备配置

入口为“设备 → 指定设备 → 插件”，用于实例配置、凭证、处理器绑定和活动查询。CLI 使用同一管理契约：

```sh
wakeit --json plugin instance --runner RUNNER_ID catalog
wakeit --json plugin instance --runner RUNNER_ID create --file instance.json
wakeit --json plugin instance --runner RUNNER_ID configure --file settings.json
```

创建参数包含 source、extension、name、config 和 grants；提供 instance_id 可在重试时保持身份。保存配置包含 instance_id、update.operation_id、expected_revision、config 和 secrets。结构化示例见[英文指南](plugin-sdk.md#device-configuration)。

秘密更新明确区分 keep、replace、clear；读取只返回是否配置。新值通过在线认证请求发送到 Runner，不保存在 Gateway 操作或事件队列中。扩展是受信本机代码，SDK 不提供操作系统沙箱。

调用固定代码版本和普通配置修订，修改不会覆盖已有投递的执行上下文。更新保留版本资源，升级验证配置和权限后再切换；失败候选不替换正常版本。更新包与激活实例是两个操作。

网页安装、更新和删除代码包需要设备所有者明确允许：在 DATA_DIR/plugins-policy.json 中设置 `{"remote_package_management":true}`。配置和实例管理不等于允许任意远程 Shell。

### 将配置型 SDK Agent 绑定到本地实例

Runner 配置 version 1 支持 `agent_instances`，将 Agent ID 显式映射到本地插件实例 UUID；该字段可省略，默认为空映射。官方 Agent 默认没有实例专属配置或凭证，因此无需创建实例或填写绑定。配置型 SDK Agent 必须显式绑定，不会按显示名称相同或列表中的第一项自动选择实例。

1. 在实际执行任务的 Runner 上安装插件，通过“设备 → 指定设备 → 插件”或上面的 CLI 命令在该设备创建实例。选择已安装的 `source` 和 `wakeit.plugins` 中的插件资源键；管理 API 将该资源键称为 `extension`。按声明配置字段和凭证槽，授予所需且已声明的权限，并启用实例。
2. 从创建响应或 `plugin instance --runner RUNNER_ID catalog` 获取实例 UUID，在该 Runner 本地 `runner.yaml` 中将其绑定到 Agent ID，不写入 Task 或 Agent 参数。
3. 保存文件后按现有 Runner 配置重载流程生效；当前 Runner 配置变更需重启 Runner。保存文件不会把已经开始的 Run 切换到另一个实例。

例如，某插件的配置 schema 声明了字符串 `endpoint` 时，CLI 创建文件可采用以下内容。请按已安装插件替换 `SOURCE_FROM_CATALOG`、资源键、配置和 grants；下列 UUID 是合成示例：

```json
{
  "instance_id": "8c311e35-4bec-4fc4-a6ae-f3654b3ff8b4",
  "source": "SOURCE_FROM_CATALOG",
  "extension": "example",
  "name": "Work account",
  "config": {"endpoint": "https://service.example.com"},
  "grants": []
}
```

配置并启用该实例后，将以下绑定合并进已有 Runner 配置：

```yaml
version: 1
agent_instances:
  example: 8c311e35-4bec-4fc4-a6ae-f3654b3ff8b4
```

这里的 `example` 是 Agent ID，不是实例显示名称。实例必须位于同一 Runner，属于该 Agent 的相同安装来源、插件资源和包版本；缺少、停用、移除或不匹配时均报错，不回退到其他实例。UUID 是开发／运维配置引用，不应加入任务指令或日常报告文案。

Runner 将非秘密配置及其修订固定在 Run 本地快照中，`config.get` 读取该快照值，而非之后编辑的最新配置。凭证不进入快照：`credentials.get` 只按需即时读取已声明的槽，并核验实例当前可用性和权限。代码升级或普通配置变化不会替换已固定的执行上下文：保存的版本资源和配置快照仍可用于旧作业与会话。升级时未变更的凭证仍可读取；轮换或清除凭证后，旧调用不能继续读取该槽，需要新的已授权调用。停用／移除实例或撤销所需权限后，即使快照仍在，也会阻止后续宿主读取；撤销不会抹除已经交给受信本机代码的值，也不会改写已有 Run 快照。

## 宿主 API 与交互

宿主调用使用具名业务方法，不开放任意携带身份的 HTTP。命名空间授权和当前工作区成员权限共同生效；用户动作保留发起者权限。插件不获取 Gateway 或 Runner Token。

SDK 提供配置、凭证槽、私有修订存储、业务查询／修改、事件、Hook、动作、Provider、外部 HTTP 和受管理的子进程。修改使用请求幂等键及适用的对象 revision。HTTP、子进程、授权流程分别要求 http:request、process:execute、auth:manage。

动作可以请求 schema 表单、选项、确认、秘密输入、授权地址、设备码及进度，由 WebUI 共享控件展示。关闭页面不代表取消；取消是明确的宿主操作。

OAuth 支持授权码＋PKCE 和设备授权，verifier 和 Token 保存在 Runner。Gateway 验证短期 state 并转发授权响应。供应商要求仍然适用；没有可达回调时需要供应商支持设备码或本机流程。Token 刷新和撤销使用明确的凭证操作。

## 通用能力验证

[`fixtures/plugins/generic`](../fixtures/plugins/generic) 是使用 TypeScript SDK 的合成插件，验证配置、凭证不回显、allow/deny/patch、宿主回调、耐久投递与重放、结构化动作和取消。不发送外部消息，不使用真实业务凭证。

完整覆盖和语义见 [RFC 4](rfc-4-plugin-sdk.md)。

## 待完成操作与公开过程流

存在远程 Hook 的工作区操作会返回 HTTP 202 和操作回执。CLI 和 WebUI 跟踪回执直到取得已提交结果；SDK 可通过 runtime.operation 查询、runtime.cancel 取消。已取消的提案不能在稍后提交，权限和 Hook 配置修订会在提交事务中再次检查。

TypeScript 使用 `ctx.streams.events(runId, {after, kind, signal})`，Rust 使用 api::EventCursor，按游标读取公开过程。这沿用有保留期的执行记录，不为每个 token 建立普通耐久回调；截断与到期标记仍然可见。完成事件继续采用耐久投递。

files.stat/read 提供修订校验的分块读取；files.watch/unwatch 仅监听明确授权的目录，消费落后时报告缺口。artifacts.register/export 建立授权引用，下载交互使用该产物 ID，不会把另一台设备的路径当作本地文件。

## 原生交互环境

`session_interactive` 返回 `InteractivePlan {cwd, argv, env}`。可选的 `env`
采用带 `source` 的引用值：`literal` 表示普通文本，`inherited` 表示本地环境变量名，
`credential` 表示已声明的凭证槽；具体文本或引用名放在 `value` 中。
凭证引用只由所属 Runner 按绑定实例解析，不通过 RPC 计划传输凭证实值，也不进入保存的会话命令。
计划不能继承或覆盖 Wakeit 的认证环境变量。

依赖环境变量的计划可以用于本机 CLI 续聊或网页终端，但不能导出为可复制的 shell 命令；
这与旧适配器 `env`／`env_from` 的边界一致。执行前仍由 Runner 核验精确会话、目录、
执行范围与占用锁，不因 SDK 计划而绕过这些限制。

对于 unknown 的 Run，可用 `wakeit run reconcile RUN_ID` 请求保存的 SDK 后端核对，
DAG 节点加 `--node NODE_ID`。REST 对应 `POST /runs/{execution_id}/reconcile`。
`ctx.runs.reconcile(runId)` 走实际观察路径，`runs.resolve` 则是另外的
`{state,reason}` 人工确认。核对、离线恢复与重启都不会重新提交 Agent 工作；
不支持或仍无法确认的结果继续保持 unknown。
