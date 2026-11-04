# Wakeit 通用插件 SDK 设计稿

状态：已批准，实施中。日期：2026-11-03。

目标是设计完整的扩展契约，覆盖 Wakeit 已有产品域及其生命周期。按依赖顺序实现不缩减公开契约，不把关键能力留作未定义的“后续扩展”。现有 Agent 接入、插件包管理与本提案的新增能力在本文中分别说明。

## 1. 运行架构和身份

插件没有邮箱、通知、同步等业务类型。一个包可以同时提供 Agent、命令和通用扩展，SDK 以资源和权限描述能力。

- **Package**：沿用现有 Git source／本地绝对路径身份，name 仅用于展示；version 与 source ref 分开。代码可由同一系统用户共享，按版本／内容存放。
- **Extension**：包内具名扩展资源，一个入口可以注册事件处理器、Hook、action、Provider 和自定义事件。
- **Instance**：某个 Runner 上的扩展实例，身份包含 Runner、package source、extension ID 和 instance ID。同包可以建立多个实例，分别保存设置、账号和订阅；不产生包别名。
- **Binding**：工作区授权哪些事件、Hook 或公开操作交给哪个实例，包含筛选、顺序、权限和故障策略。
- **Invocation**：一次具体调用，关联事件投递、Hook 操作或 action，有稳定 ID、版本、配置修订及 deadline。

所有第三方代码都在 Runner 管理的独立子进程中执行。Gateway 负责业务权威、授权、订阅、Hook 操作和事件路由，不在其进程中加载第三方代码。工作区级 Hook 通过指定 Runner 执行；工作区管理员能在界面中看见这项设备依赖。需要持续可用的业务策略可绑定常在线 Runner。

每个启用实例有一个扩展进程，使用双向 stdio JSON-RPC。宿主支持并发请求关联、取消与结构化错误；stdout 只承载协议，stderr 进入实例日志。宿主不使用 Rust 动态库 ABI，也不内嵌 Node。提供同等语义的 TypeScript 和 Rust SDK，以及语言无关协议／JSON Schema；脚本声明运行时，预编译插件沿用平台 artifact 分发。

当前包安装、资源校验、不可变版本目录、来源选择、Agent YAML 和命令执行继续复用。现有 adapter-only 包不需要启动扩展进程。新增 manifest 能表达无 Agent 的包，但至少包含一种资源。

## 2. 三种契约

| 契约 | 含义 | 是否能影响原操作 |
|---|---|---|
| `on(event, handler)` | 观察已提交的业务事实 | 原操作不等待；处理失败独立重试，不能撤回事实 |
| `hook(stage, handler)` | 对尚未提交的操作提案进行校验、拒绝或变换 | 宿主等待；只能返回该阶段明确允许的字段 |
| 固定宿主回调 | 插件启停、配置校验／迁移、action、Provider 等确定调用点 | 按各回调契约处理，不由插件冒发核心事件 |

事实记录、进程取消、设备撤销、权限撤销和故障核对始终由核心决定。完整生命周期可观察，不代表这些事实可以被否决或改写。SDK 不提供 setRunState、raw SQL、总线凭据或裸宿主 Token。

## 3. 事件目录

所有域复用统一信封，目录中的事件名为提案。事件只在实际操作或实际变化时产生；查询不会制造假“发现”或状态变化。产品没有的操作不凭 SDK 名称自动增加，例如 Task 当前只有归档／恢复，没有物理删除 API。

| 域 | 事件 | 语义和来源 |
|---|---|---|
| 工作区 | `workspace.created`, `workspace.settings_changed` | Gateway 已提交的工作区信息／设置变化 |
| 成员 | `member.joined`, `member.left`, `member.role_changed` | Gateway 已提交的成员变化，不含登录 Token |
| Task | `task.created`, `task.updated`, `task.enabled_changed`, `task.schedule_changed`, `task.archived`, `task.restored` | 一次修订可以产生不同语义事件，使用共同 operation ID 关联 |
| Template | `template.created`, `template.updated`, `template.archived`, `template.restored`, `template.instantiated` | 已保存模板或已创建实例任务；普通预览归交互流 |
| 调度 | `schedule.occurrence_created`, `schedule.evaluated`, `schedule.next_changed` | 每次逻辑 occurrence 有稳定身份；记录 misfire／overlap／准入结果，不按扫描 tick 产生事件 |
| Run | `run.created`, `run.queued`, `run.started`, `run.state_changed`, `run.cancel_requested`, `run.retry_requested`, `run.reopened`, `run.uncertain`, `run.reconciled`, `run.finished` | Gateway 权威状态，普通父运行与 DAG 父运行一致 |
| DAG | `dag.node.ready`, `dag.node.state_changed`, `dag.node.retry_scheduled`, `dag.node.attempt_created`, `dag.node.attempt_finished`, `dag.node.finished`, `dag.recovery_planned`, `dag.recovered` | 节点、attempt 与整图是不同对象；整图完成统一用 run.finished |
| 执行 | `execution.claimed`, `execution.prepared`, `execution.started`, `execution.input_submitted`, `execution.native_finished`, `execution.cancel_requested`, `execution.cancel_confirmed`, `execution.uncertain`, `execution.reconciled`, `execution.finished` | Runner 本机执行事实；native_finished 与结果后处理完成分开 |
| 过程 | `execution.message_updated`, `execution.tool_updated`, `execution.log`, `execution.recording_changed` | 公开内容和工具状态；不转发隐藏思考。高频内容流按游标读取，见第 4 节 |
| 会话 | `session.created`, `session.bound`, `session.title_changed`, `session.availability_changed`, `session.continuation_started`, `session.continuation_finished` | 真实会话操作和验证后的变化；list／inspect 本身不制造会话 |
| 结果 | `result.native_available`, `result.published`, `result.derived_published`, `result.processing_failed`, `result.expired` | 不可变版本引用，保留原生来源、格式和完整性信息 |
| 文件／产物 | `artifact.registered`, `artifact.exported`, `artifact.expired`, `artifact.removed`, `file.changed` | 宿主管理的产物；file.changed 仅限显式登记的文件监听，非全磁盘监控 |
| 设备 | `device.enrolled`, `device.online`, `device.offline`, `device.capabilities_changed`, `device.lease_lost`, `device.lease_recovered`, `device.revoked` | 实际变化产生事件，心跳包不逐个广播 |
| Agent／目录 | `agent.availability_changed`, `agent.catalog_changed` | 检测／目录摘要确有变化时产生，不臆造模型或会话能力 |
| 插件 | `plugin.installed`, `plugin.updated`, `plugin.removed`, `instance.activating`, `instance.activated`, `instance.deactivated`, `instance.failed`, `instance.recovered`, `instance.config_changed`, `instance.permissions_changed` | 区分包安装与实例生效，事件不含凭证值 |
| 授权／动作 | `credential.status_changed`, `auth.flow_changed`, `action.started`, `action.progress`, `action.finished` | 仅对实例和已授权用户可见，不进入全工作区公开广播 |
| 操作／投递 | `operation.state_changed`, `delivery.state_changed` | 用于 UI 和诊断；默认不触发同一处理器自身的递归监听 |
| 自定义 | `plugin:<source>#<extension>/<event>` | 插件只能发布自己命名空间中的、已声明 schema 的事件 |

### 状态和触发规则

1. 对象变化、语义事件和路由待处理记录在同一权威事务中提交；不能只在某个 HTTP handler、SSE 流或 Runner 上传路径补事件。
2. `run.created` 总是产生。创建即 skipped／cancelled 时直接产生相应 state_changed 和 finished，不虚构 queued／started。
3. unknown 非终态，不能产生 finished。核对后先记录 reconciled，真实结算再产生 finished。
4. cancel_requested 是意图；cancel_confirmed／最终 cancelled 才表示执行已确认终止。
5. run.started 对应核心运行计时窗口开始：普通运行首次领取、DAG 首个节点领取并进入执行准备。只有 execution.started 确认原生命令已启动，不能把排队或领取伪装成原生启动。
6. 节点 attempt 失败但仍计划自动重试时，只结算 attempt，不提前结算节点或父 Run。
7. 整轮重试产生新 Run；同轮节点恢复沿用父 Run ID，并增加恢复代次。每次确定结算产生一个新的 run.finished。
8. 一次执行在准备阶段被拒绝时可直接结束，没有 started；事件明确 started=false。原生进程未确认结束时不发布 execution.finished。
9. 最终状态、输出完整性、Session ID／cwd 等只由真实协议和核心校验确定，插件不能通过 emit 冒发核心事件。
10. 高频读取／预览／文件访问进入可选 audit／interaction 流，避免普通页面浏览触发业务通知。

## 4. 事件账本、数据与交付

### 信封

```text
event_id, event_type, schema_version
workspace_id
aggregate: {type, id, revision}
occurred_at, committed_at
operation_id, correlation_id, causation_id
origin: {kind, actor_id?, plugin_instance_id?}
source_runner_id?
task_id?, run_id?, parent_run_id?, node_id?, attempt_id?, recovery_generation?
snapshot_ref?, data
```

- Gateway 业务事实保存在专门的业务事件账本；本机事实先入 Runner Journal，可靠上传后获得工作区交付。现有 Run 日志／Trace 不充当全产品业务账本。
- 同一聚合 revision 单调递增；不承诺跨任务的全局业务顺序。事实唯一键包含对象、阶段和恢复代次。
- 事件事务创建待路由记录，dispatcher 扫描未处理记录；不能以简单的自增 ID 高水位扫描假设提交顺序。
- 完成快照包含当次任务显示信息、状态、输出选择和不可变结果版本。DAG 恢复清空当前结果，不改变之前的完成快照。
- 总线消息只带身份、小元数据和引用，大正文分块读取。结果保持 text／Markdown／JSON 等实际格式，带 complete／truncated／expired 标志和到期时间。
- 元数据、正文、文件各自遵循明确保留期。投递有 expiry，永久离线不能无限延长内容保留；缺失／到期返回明确错误，不伪装成空结果。

### 订阅与处理

订阅记录目标实例／handler、事件与 schema 范围、workspace／任务／状态／来源设备／parent-node-attempt 筛选、可读数据字段、顺序、重试和到期策略。

- 默认只接收启用后的新事件。历史补发是显式 replay operation，固定事件集合，展示预计数量。
- 正常交付唯一键为 subscription + event ID。手动重试保持同一 delivery ID；显式重放创建新的 delivery ID 和 replay ID，仍引用原 event ID。
- SDK 暴露 event.id 和 delivery.idempotencyKey；后者在同一次逻辑交付的所有重试中稳定，显式重放可形成新外部效果。
- 同一 subscription／aggregate 保序，不同聚合可并发；一个插件故障不阻塞其它插件或 Agent 的执行容量。
- 至少一次投递。handler 返回 success、retry_after、permanent_failure 或 uncertain；记录完成后确认消息。重试包含最大次数、退避与截止时间。
- 外部发送成功与本地确认之间崩溃仍可能重复。非幂等副作用遇到不确定状态时进入 uncertain，插件核对或用户确认后继续；不能把 SMTP 描述为恰好一次。
- 订阅停用后停止新路由；已有交付暂停并保留到期信息，用户可以恢复或明确丢弃。取消订阅不撤回已发送邮件。
- 授权在绑定、投递和读取正文／文件时复核。权限撤销后不能靠旧队列继续读数据。
- 每次调用记录所用代码版本和普通配置 revision；重试沿用该次逻辑交付的执行上下文。显式重新处理可选择当前配置并产生新操作，不悄悄更换收件人或版本。

### 高频过程订阅

message／tool／log 通过独立的流 API 订阅，有 sequence、resume cursor、内容保留期和 gap/truncated 标记。不会为每个 token 创建普通业务投递，也不让慢监听器阻塞原生输出读取。断开后按保留游标恢复；超过保留期明确报告缺口。业务级 finished 仍走耐久交付，两者语义分别可测试。

自定义事件继承同样信封、schema、路由和授权机制。origin／causation 链贯穿宿主 API；订阅可选择是否接收自身导致的事件。宿主限制因果递归深度并明确报错，避免多个自动化插件无限触发。

## 5. 生命周期 Hook 目录

Hook 输入均为不可变操作提案，输出为 allow、deny 或该阶段的受限 patch。不是每个数据库读写都插一个 Hook。

| 阶段 | 权威位置 | 可验证／可修改内容 | 固定边界 |
|---|---|---|---|
| `workspace.settings.prepare` | Gateway | 用户有权编辑的业务设置候选 | 不改变身份、成员授权或认证政策 |
| `task.save.prepare` | Gateway | 保存前 Task／DAG 候选定义，包括尚未冻结的图、调度和执行参数 | 不改 workspace、Task ID、创建者或 revision；重新执行原生完整校验 |
| `task.activation.prepare` | Gateway | 启用、归档／恢复等业务准入 | 用户停用、取消和撤销类控制不接受否决 |
| `template.save.prepare` | Gateway | 模板定义、参数 schema 与蓝图 | 不绕过类型／绑定／图结构验证 |
| `template.compile.prepare` | Gateway | 类型化参数覆盖、绑定候选 | 不直接操作 MiniJinja 内部对象 |
| `template.compile.result` | Gateway | 尚未创建 Task 的编译结果 | 预览和正式创建采用同一提案及版本；创建前复核 |
| `schedule.fire.prepare` | Gateway | 一次已确定 occurrence 的准入、该次允许的输入覆盖 | 核心控制 Cron／时区、next_at、misfire、overlap 和 occurrence 去重 |
| `run.create.prepare` | Gateway | 冻结前的有效执行输入、创建业务准入 | 不改来源身份、Run ID、工作区与已有历史 |
| `run.retry.prepare` | Gateway | 整轮重试的业务准入和新候选输入 | 不修改旧 Run，返回清晰的新轮操作 |
| `dag.recovery.prepare` | Gateway | 节点重试／整图恢复的候选计划、业务准入 | 核心确定受影响后继、成功节点保留和恢复代次 |
| `dag.node.dispatch.prepare` | Gateway | 该次节点就绪后的输入覆盖／准入 | 图已冻结，不改依赖、输出选择、路由、session 或节点身份 |
| `execution.prepare` | Runner | 实际提交前 prompt 和已声明参数 | Runner／Agent／cwd／session／deadline 固定，变换后验证并保存有效快照 |
| `session.bind.prepare` | Runner | 精确绑定的业务校验 | 不能改成 last／continue 或替换 Session ID；目录和锁由核心验证 |
| `session.continue.prepare` | Runner | 续聊输入校验与允许的 prompt／参数修改 | 不偷偷替换原会话、解锁或重命名已有原生会话 |
| `session.title.prepare` | Runner | 新会话生成标题或无标题映射的候选 | 不覆盖已有原生标题／已保存映射；显式人工改名仍受原生能力约束 |
| `result.publish.prepare` | Runner | 对成功原生结果的正文／JSON／展示元数据／产物引用变换 | 原生结果不可变，原生失败不能改成成功，完整性标记不可洗掉 |
| `artifact.export.prepare` | 源 Runner | 导出业务准入、元数据和展示方式 | 不放宽 allowed_roots、源对象身份或访问授权 |
| `action.prepare` | Runner／Gateway 协调 | 已声明用户动作的输入与业务准入 | 不能提升发起用户／实例权限；交互由 action 协议承接 |

对于保存后的变更、进程结束、取消请求／确认、撤销、租约失效、保留期清理等，完整提供事实事件和有时限的清理回调。插件不能否决必须发生的核心操作。

### 顺序和统一错误语义

- 每个 stage 由宿主保存明确 binding 顺序；同作用域串行，将前一个 patch 应用于下一个候选。不依赖安装时间、HashMap 或回包顺序，不再另建 Hook 依赖 DAG。
- workspace 绑定与明确授权的任务级绑定形成可见的有效链。每次操作固定链、代码版本、config revision 和授权 revision。
- deny 带稳定 code、用户消息及可选字段路径，始终拒绝本次操作。
- 故障／超时／离线使用管理员为 binding 选择的 reject、wait-until-deadline 或 continue 策略；默认 reject。continue 必须显式配置并记录本次跳过，不覆盖正常业务 deny。
- prepare Hook 是决策／变换契约，不能在返回前执行不可回滚业务效果。Hook context 仅有授权读 API 和返回提案的能力，禁止递归执行写 API；通知等副作用使用提交后的事件／独立 action。
- deadline、显式取消、Runner shutdown 通过 cancellation token／AbortSignal 传递；超出退出宽限则终止实例及它拥有的子进程，恢复实例并标明相关调用失败／不确定。
- Hook 用时计入所属 operation／执行时限。输入 Hook 失败明确记录阶段及插件，不借用原生进程不确定性 unknown 状态。

### 结果后处理

native_finished 和原生结果先保存；result.publish.prepare 顺序产生 effective result，并记录变换来源。DAG 后继使用已发布的有效结果，仍执行非空、完整性和类型校验。

变换失败保留原生结果，Run 使用现有 failed 状态并标记 failure_phase=postprocess，不将原生成功说成原生失败。可单独重试结果处理，复用该次原生结果；不能因此默认重跑已有外部副作用的 Agent。取消和处理超时仍遵守原执行时限。raw／effective／derived 结果拥有不同版本引用；事后派生报告不改写已冻结的 DAG 结果或历史完成事件。

## 6. Gateway 权威 Hook 的完整执行流程

工作区 Hook 代码仍运行在指定 Runner。需要新增可恢复操作记录，不允许持数据库锁等待设备。

1. 先认证／授权和规范化候选，建立 operation ID、proposal revision、对象 revisions、输入摘要与 deadline。
2. 记录有效 Hook 链及固定执行上下文，再通过设备通道调用目标实例。此时不持 Task／Run 业务行锁。
3. 每个 invocation 绑定 operation、stage、handler、input digest、配置／代码版本和 deadline。Runner 持久化结果，重复请求复用已完成决定；迟到回复不能作用到新提案。
4. 完成后在短事务中重新核对权限、对象 revisions、设备身份和 binding revisions，对最终候选执行全部核心验证，再 CAS 提交并写事实事件／路由记录。
5. 同一操作只有一个 Gateway 能提交。冲突返回明确 operation 状态；新的候选修订需要新的调用，不拿旧 patch 覆盖新定义。
6. HTTP 可等待短时结果，未完成返回 202 和 operation ID；WebUI／SDK 可 watch、查询或显式取消。网络断开不等于操作撤销。
7. 调度器以 occurrence ID 原子预留一次到期操作并推进扫描游标，随后异步准入；不会拿任务锁等待 Hook 或每轮扫描重复调用。准入完成后重新检查任务启停／修订、overlap、start_grace 等，幂等创建 queued／skipped／failed 的运行。
8. 指定 Runner 离线时按 binding 策略等待、拒绝或明确跳过。这项依赖显示在 WebUI；不通过隐式其它设备抢占或共享凭证提供假容灾。

operation 状态统一为 pending、waiting_hook、waiting_device、validating、committed、rejected、conflict、cancelled、expired、failed，带阶段、可重试性和稳定错误码。没有活跃 binding 的普通操作仍走原有直接业务路径，不强制增加一轮网络调用。

## 7. 宿主 API 覆盖面

所有核心公开业务 API 有类型化 SDK 对应入口，遵循同样的权限、输入验证、幂等和审计路径；不通过通用 request('/anything') 暴露内部端点。查询分页／cursor、watch／取消、一致的类型和错误由 SDK 封装。

| 命名空间 | API 面 |
|---|---|
| `host` / `context` | 宿主及协议版本、平台、当前实例／workspace／actor、已授予能力、invocation、deadline、signal |
| `workspace` / `members` | 工作区信息／设置、成员查询与受授权的成员管理；不返回用户密码／登录 Token |
| `tasks` | list/get/validate/create/update/enable/disable/archive/restore/trigger；与现有业务保持一致 |
| `templates` | list/get/validate/create/update/archive/restore/preview/instantiate；暴露编译收据与预览身份 |
| `schedule` | preview、下一次执行和 occurrence 查询；变更经 Task 修订操作提交 |
| `runs` | list/get/watch/events/cancel/retry/reconcile；状态和结果只通过核心业务操作改变 |
| `dag` | 读取图／节点／attempt、输出选择、恢复预览、节点重试和恢复；不提供写运行中依赖图的捷径 |
| `executions` | 当前快照、公开进度、取消状态、实际执行诊断；取消仍走拥有者与核心语义 |
| `agents` / `models` | 实际能力与检测状态、模型目录查询；不以 SDK 静态列表兜底 |
| `sessions` | list/inspect/bind/continue/rename（能力允许时）；维持精确 ID、cwd、会话锁和命名规则 |
| `results` | 不可变 raw/effective/derived 版本、parts、stream、完整性与到期；发布派生产物与当前结果处理返回分开 |
| `files` / `artifacts` | stat/open/stream/register/export/watch/unwatch；引用按源 Runner／目录权限解析，临时文件跟随调用清理 |
| `devices` | list/get/capabilities、受授权的登记／设备管理；插件不掌握 Runner 的原生凭证 |
| `plugins` | 目录／资源／实例查询、安装／更新／启停／卸载／健康／授权；影响其他 Runner 的操作显式列出范围 |
| `config` | schema/defaults/get/validate/save/status/watch，expectedRevision 与生效 revision 明确 |
| `credentials` | 实例凭证槽的 status/set/remove 与本机受授权读取；WebUI 永远不读取原值 |
| `storage` | 实例私有持久 KV、list/get/set/delete/CAS 与小事务，独立于包目录与 Run 保留期 |
| `auth` | API key／OAuth 授权流程、status/cancel/revoke、refresh、账号引用；用户凭证不通过 UI 回显 |
| `events` | 声明／监听／取消监听、自定义 emit、历史查询、交付状态、retry/replay；绑定需管理员授权 |
| `hooks` | 声明 handler／stage／输入输出契约；返回 allow/deny/patch，不直接写被拦截对象 |
| `actions` / `ui` | 注册动作、输入输出 schema、进度、表单／选择／确认／凭证／授权／下载／导航、取消 |
| `providers` | 注册 models/sessions/artifacts 等明确查询 Provider，返回标准 DTO；Agent 执行仍使用其现有声明协议 |
| `http` / `process` | 插件外部服务请求、受宿主管理的子进程、流、deadline 和取消；不传宿主身份 Token |
| `log` / `health` | 结构化日志、稳定错误 code、健康报告、脱敏字段和关联调用；插件状态不混入 Agent 检测状态 |

mutation 接受 requestKey，适用对象带 expectedRevision，统一返回 operation receipt；业务型同步完成也返回已提交收据。用户 action 取用户权限与实例授权的交集，后台事件使用实例服务身份。权限每次调用检查，不长期借用安装者的登录 Token。

查询 Provider 必须只读。models/sessions Provider 不得隐式推理或创建运行；需要真实会话的操作使用 sessions API。注册是按完整 provider 资源 ID 授权和显式选择，不能以加载顺序覆盖现有适配器。

SDK API 命名和 DTO 由同一契约生成 TS／Rust 类型；API 文档逐项标注 authority、required grants、side effects、allowed contexts、request/response、error codes、timeout/cancellation 和事件结果。实现验收逐项对照此表，不留空壳或静默 no-op。

## 8. 权限、配置、凭证和本地数据

### 权威归属

- Gateway：workspace 授权、binding／订阅期望状态、operation／事件／路由／交付元数据。
- Runner：实例业务设置、凭证、私有 storage、实际加载版本和已应用 revision。
- 包缓存：代码和静态 schema／能力描述，可以系统用户级共享；数据永不保存在包版本目录。
- 浏览器：未提交普通表单草稿；凭证仅保留内存，不自动保存到浏览器缓存。

插件声明能力不等于获得权限。静态能力清单和 runtime 注册需要一致；启用时管理员授予具体作用域和字段访问，新版本扩大权限需要重新授权。viewer 查看基本状态，operator 在授予及自身权限范围执行公开 action／查看内容，admin 管理配置／凭证／binding／安装。设备本地策略决定是否允许远程安装或管理；workspace admin 不自动拥有系统用户全部权限。

插件具有本机用户的实际进程权限；SDK 命名空间是契约和误用防护，不是 OS 沙箱。宿主不向插件注入 Runner／Gateway Token 或其它实例配置。插件文件、网络访问仍按本机权限运行，不能声称声明权限能约束恶意原生程序。

### 配置保存

saveConfig 使用 operation ID、expectedRevision、普通字段和 secretUpdates。Runner 执行 schema／插件业务校验，原子保存，再应用新 revision。验证失败保留旧配置，应用失败标记“已保存／未生效”并提供恢复操作，不能误报成功。工作区 binding 和设备设置分别有保存结果，不构造跨两处存储的假原子提交。

秘密更新为 keep／replace／clear，读取只返回 configured／expired／needs_authorization。新秘密经在线短期请求传送到 Runner；Gateway 不将值持久化到业务表、Outbox、event 或操作日志。超时先查 operation receipt，不能把空框当删除或盲目重发。私有文件按现有系统用户权限保护；不声称链路转发等于端到端不可见，也不声称本地明文文件是密码学隔离。

Runner 离线时可以保存 Gateway 上的订阅期望状态；设备设置／凭证只能保留临时草稿，等上线确认后提交。离线修改订阅显示等待设备，不能等同实例已激活。撤销 binding 可在 Gateway 立即停止新投递／调用。

## 9. 结构化 Action 与授权

声明 action ID、名称／说明 i18n、输入／输出 schema、权限、外部副作用说明、取消能力及展示方式。WebUI 和 CLI 调用同一个逻辑 action；现有 executable commands 不自动变成网页任意命令执行器。

一次 action 用 operation/invocation ID 贯穿以下交互：

```text
progress ↔ form / choice / confirm / secret
         ↔ open_url / device_code / authorization
         → completed / failed / uncertain / cancelled / expired
```

- 表单与选择由共享 schema 控件渲染，确认明确实际影响，秘密输入不进入事件日志。
- Toast、导航、下载等只发给当前交互用户。无人值守 handler 不能无限等待一个不存在的用户；返回 interaction_required，转入可由管理员处理的 action。
- OAuth 支持 authorization-code + PKCE 和 device-code：Runner 保留 verifier／Token，Gateway 验证短期 state 并转发回调，浏览器显示授权 URL／验证码和进度。Token 不进 URL 或 UI 回显。
- 公网回调不可达的部署可使用供应商支持的 device-code 或明确的本机授权流程。协议准确返回 unsupported_flow，不把任意 OAuth 宣称为可用。
- Token 刷新、失效、撤销及账号删除有明确状态；更新凭证槽后刷新 UI，不把 Token 当普通配置同步。
- 取消操作真实传到插件并终止后续流程；关闭页面不擅自等于取消已接受操作。UI 提供显式取消和稍后查看状态。

## 10. 文件与跨设备结果

文件引用包含 workspace、source Runner、result/artifact 身份和允许操作。目标插件不能把另一台设备的绝对路径当本地文件。

open/stream 经宿主向源设备请求，核对授权、allowed_roots、真实路径、文件版本和保留期，分块传输到调用管理的临时资源。源设备离线、文件变化、删除或过期返回准确状态；插件可以等待、只发送链接或报告失败。

新生成的插件产物登记为带来源和完整性信息的 artifact；对外导出需要对应授权。files.watch 只监视显式获准的目录，包含启动、停止、事件游标和缺口语义；不能用一个目录引用读遍整台设备。

## 11. 插件自身生命周期和管理

固定回调包括 initialize/setup、validateConfig、applyConfig、migrateConfig、activate、health、deactivate 和 dispose。每个回调定义输入／输出、deadline、取消及失败后状态。

1. **安装**：解析 source/ref，下载／校验 artifact，检查宿主协议和运行时，保存静态 schema／能力；不执行任意安装 hook，不自动启用全部能力。
2. **创建实例**：确定目标 Runner 与独立数据目录，保存配置、凭证和 grants，形成可审计的实例身份。
3. **激活**：启动进程、协商协议、核对注册项和配置，完成后才显示运行中；无效注册或缺少权限显式失败。
4. **更新**：暂存版本，查看 schema／权限变化，迁移普通配置并验证，原子切换实例 generation。新权限未授予前不得静默生效。
5. **在途调用**：固定代码／普通配置 revision，旧进程在调用完成后退出；需强制更新时明确取消在途调用。新版本切换失败保留可恢复的旧版本和配置。
6. **停用**：停止新调用，Gateway 解除／暂停对应 binding，在途执行按明确的 drain 或 cancel 策略处理。插件不能否决自身停用。
7. **卸载**：先处理实例／Agent／会话引用，列出受影响范围；普通卸载保留私有数据，清除数据是独立操作。仍被执行快照或会话入口引用的旧代码不删除。
8. **崩溃恢复**：宿主记录故障并退避重启，只恢复未确认的合法投递；正在运行的 Hook 不伪造 allow。超出重启预算显示故障而非拖垮 Runner。
9. **远程管理**：CLI 与 WebUI 使用同一管理契约。远程安装／更新需设备本地允许及管理员授权，显示共享包影响的 Runner；仅切换所选实例的 active version，不能隐式切换其它工作区。
10. **宿主退出**：停止新调用，发 cancellation，短时 dispose，清理拥有的子进程。活动调用、持久交付和确认结果在下次启动后可核对恢复。

## 12. WebUI

入口为设备 → 选定设备 → 插件 → 插件实例。设备详情中的插件 Tab 与概览／Agents 同级；内部复用紧凑列表、统一大卡片与共享字段控件。

详情包含配置、能力与授权、活动三个视角：

- 配置：schema 表单、凭证状态／更换／清除；分别显示设备设置与工作区 binding 的保存／生效状态。
- 能力与授权：可搜索的事件、Hook、action、Provider 清单；显示绑定范围、承载设备、有效 Hook 顺序、超时／离线策略，用户不编辑协议 JSON。
- 活动：同一列表展示 invocation／operation／delivery，包含源运行链接、时间、尝试次数、准确状态、错误和允许的重试／核对／重放操作。
- 管理动作：安装、创建实例、配置、授权、启停、更新、版本状态、卸载；有副作用动作明确写实际名称，例如发送测试邮件。
- 主状态只突出一个：未配置、停用、启动中、运行中、待生效、故障、离线。详情可查安装版本与运行版本，不堆徽章。
- 所有选择器、Tooltip、Toast、层级表单、结构化动作弹层采用现有组件，zh/en 同步；未操作前不连续报错。插件不注入任意 HTML／JS 组件。

## 13. 协议和兼容性

Agent 配置和 Agent events 保持 version 1。通用扩展 IPC、事件 schema、宿主 API 各自带明确版本和能力协商；新版 manifest 独立标识 SDK 资源。现有包资源继续按其 manifest 支持范围读取，旧宿主遇到新 SDK 包明确报不兼容，不忽略它后声称安装成功。

插件声明宿主协议／API范围、平台／运行时要求、资源 ID、配置 schema 版本、所需权限、注册事件／Hook／action／Provider schema。静态描述可离线展示；runtime 注册必须匹配，不接受加载后偷偷新增权限。

无能力的方法返回 unsupported，不空返回成功；数据到期返回 expired，修订冲突返回 conflict，未授权返回 forbidden，设备离线／调用超时／业务拒绝／外部不确定分别有独立 code。SDK 封装统一 cancellation、分页、stream 和 operation receipts。

## 14. 验收覆盖

验收针对协议与业务效果，不使用像素位置或实现镜像测试。

- 各域实际状态写入路径均产生正确事件：包括直接 skipped／cancelled、unknown／核对、自动 attempt、同 Run 恢复再完成、归档／撤销批量状态变化。
- 各 Hook 的允许 patch、拒绝、串行顺序、核心复核、CAS 冲突、取消、离线策略、超时及迟到回包。
- 多 Gateway 和断线重复请求不会重复提交业务操作；不持数据库行锁等待插件。
- 输入快照、原生结果、有效结果与历史完成快照正确区分；后处理重试不重复执行原生 Agent。
- 事件交付的重试／uncertain／手动核对／暂停／到期／重放；DAG 整图一次完成事件与节点事件可分开选择。
- 跨 Runner／workspace 的授权、配置、凭证及文件边界；撤销授权后在途队列不越权读取。
- 公开过程和大结果的流式读取、缺口／截断、跨设备文件离线／变化／到期。
- WebUI schema 表单和 revision 保存、秘密 keep/replace/clear、OAuth 两种流程、action 进度／交互／取消、重连后的 operation 状态。
- 扩展崩溃重启、独立重载、版本切换、配置迁移失败恢复、权限增加、卸载引用保留。
- TypeScript 与 Rust SDK 对同一协议通过相同的契约测试。
- 通用合成扩展：在 WebUI 配置普通字段与凭证，订阅事件，验证筛选、顺序、断网、重复交付、取消和显式重放；不连接邮箱或其它业务服务。
- 通用合成策略扩展：覆盖各 Hook 的允许、拒绝、受限变换、CAS 冲突和取消，以及输入／结果快照；不以特定业务插件作为验收条件。

交付物包括：插件 manifest/schema、事件目录/schema、逐项 Hook 契约、IPC 协议、宿主 API 和 TS/Rust 类型、配置／授权／交互协议、作者文档及 TypeScript／Rust 通用合成插件。上述能力均为设计和验收范围；执行可以分依赖步骤，不能用少量空接口代替完整契约。
