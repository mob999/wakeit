# 配置与执行协议 v1

## Runner 配置

注册命令生成 `runner.yaml`。相对路径以配置文件所在目录为基准。

```yaml
version: 1
gateway: https://wakeit.example.com
id: 设备注册后获得的 UUID
name: dev-server
token_file: device.token
nats_url: wss://mq.example.com
credentials_file: runner.creds
data_dir: data
concurrency: 2
agents:
  - agents/codex.yaml
allowed_roots:
  - /srv/projects
```

`allowed_roots` 检查 Task 工作目录的规范化路径，防止误选目录；它不是 Agent 的操作系统沙箱。Agent 本身的文件和工具权限由该 Agent 配置决定。

本地执行记录使用 SQLite WAL 和同步落盘。Run 保存命令配置快照和外部任务 ID，配置更新不修改已经开始的执行。当前 Runner 配置变更需重启生效。

## 命令定义

`submit`、`status`、`cancel`、`detect`、`models` 和 `session.inspect/resume/interactive` 使用相同结构：

| 字段 | 行为 |
|---|---|
| `command` | 可执行文件或静态解释器，不允许模板变量 |
| `args` | 独立参数数组，模板展开不会再次解释变量内容 |
| `stdin` | 可选标准输入文本，写入后关闭 |
| `cwd` | 可选命令目录，默认 Task.cwd |
| `env` | 本地配置的环境变量值，可以使用模板 |
| `env_from` | 子进程变量名到本机变量名的映射 |
| `timeout` | 单次命令上限，如 `10s`、`2m` |

时长支持 1 秒至 366 天。同步命令默认使用 Task 超时；异步 submit 默认 60 秒、status 10 秒、cancel 30 秒。

`output.format` 支持 `text`、`json`、`jsonl`、`events`。JSONL 结果作为 JSON 数组；`output.result_pointer` 可从 JSON 结果中提取字段。同步命令成功由零退出码及有效的结果格式共同决定。普通 stdout 仅保留最多 1 MiB；JSON/JSONL 在持续排空输出管道时对完整内容做有界流式语法校验，不将截断前缀误判为无效 JSON。有效的大型输出保留真实退出状态，结果改为带 `truncated: true` 的 64 KiB 预览；文本超限也明确标记。截断结果不应用 `result_pointer`，也不能作为完整的 DAG 上游输入。真正损坏的 JSON/JSONL 仍报告结果格式错误。

## 同步 Agent 事件流

`output.format: events` 在执行期间解析 stdout JSONL，每条记录仅使用 LF 分帧，支持 CRLF 和跨读取块的 UTF-8。原生 CLI 的输入记录与传输记录分别限流：原生 JSONL 单条最多 64 MiB，先解析 Session、执行终态及结果，再把过大的 Trace 或结果正文转成明确标记的预览。不会仅因工具输出超过传输容量就丢弃整个原生终态。规范化后一个传输事件最多 1000 KiB，为 NATS 的 1 MiB 上限预留封装空间。事件版本为 1：

```json
{"version":1,"type":"trace","data":{"id":"command-1","kind":"command","status":"running","title":"检查仓库","input":{"command":"git status"},"output":null,"occurred_at":"2026-10-14T01:00:00Z","started_at":"2026-10-14T01:00:00Z"}}
{"version":1,"type":"trace","data":{"id":"command-1","kind":"command","status":"succeeded","title":"检查仓库","input":{"command":"git status"},"output":"working tree clean","occurred_at":"2026-10-14T01:00:01Z","started_at":"2026-10-14T01:00:00Z","finished_at":"2026-10-14T01:00:01Z"}}
{"version":1,"type":"log","data":{"stream":"stdout","text":"检查完成\n"}}
{"version":1,"type":"result","data":"# 检查结果\n\n仓库状态正常。"}
```

Trace 的 id 在一次 Run 内稳定，parent_id 可选，仅使用来源实际提供的关系。kind 为 message、tool、command；status 为 running、succeeded、failed、cancelled。每次更新是完整快照，包含输入、输出和已知的起止时间；没有结束事件则保持未结束，缺少起止时间不计算耗时。occurred_at 是发生时间，Gateway 在 data.event_time 中保留传输事件时间；入库 created_at 用于游标和保留策略，不计算步骤耗时。

result 单独保存为最终报告，最后一个 result 记录覆盖前值；它不应包含完整事件流或工具输出。Run 的终态依据命令退出码、超时和终止确认，Trace 不设置 Run 状态。纯文本不猜测工具事件，异步适配继续使用 status/result，不接受 events 格式。

内置 YAML 使用 `wakeit plugin exec git:github.com/mob999/wakeit adapter -- stream <codex|claude-code|pi|opencode|kimi-code|agy|gemini-cli> -- <可执行文件> <独立参数>`。辅助命令继承 Runner 的进程组/Windows Job Object，stdin 和 stderr 直接传递；内置或第三方适配使用同一事件解析器。辅助命令保留原生非零退出码；原生明确失败却返回 0（例如 Pi JSON 模式）时返回 1，保持失败的非交互执行语义。它忽略推理内容，只转换公开消息与工具结果。Pi 的消息增量先合并为完整快照，实时快照最多约每 100ms 一次，结束快照立即回传。协议依据：[Codex](https://developers.openai.com/codex/noninteractive/)、[Claude Code](https://code.claude.com/docs/en/headless)、[Pi](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/json.md)、[OpenCode](https://opencode.ai/docs/cli/)。

日志和 Trace 共用默认 20 MiB 容量及 7 天内容保留期限，结果单独受消息上限保护。Journal 与 Gateway 在超额时记录 trace_status: truncated；损坏/未知事件记录 incomplete 并继续读，既不重新执行，也不据此改变 Run 终态。过期清理删除内容，保留 trace_status: expired，直到 Run 的 90 天记录期限结束。原生事件没有结束记录时不会根据进程退出补造工具成功。

[events-agent.yaml](../examples/events-agent.yaml) 与 [events-adapter.py](../examples/events-adapter.py) 展示第三方同步接入，无需修改 Rust 核心。使用 `wakeit schema stream` 获取完整事件 JSON Schema。

## 异步适配

`mode: async` 必须配置 `status`，可配置 `cancel`。标准输出只放协议 JSON，诊断信息写 stderr。

提交：

```json
{"job_id":"remote-job-123"}
```

查询：

```json
{"state":"running","message":"正在分析"}
```

终态：

```json
{"state":"succeeded","result":{"summary":"检查完成"}}
```

状态集合为 `queued`、`running`、`succeeded`、`failed`、`cancelled`。查询命令非零退出、格式错误或网络异常不意味着外部任务失败；连续失败后显示 `unknown`，继续退避查询。取消命令成功只表示请求已发送，最终以 status 为准。

建议外部服务以 `WAKEIT_RUN_ID` 作为幂等键。提交成功与返回任务 ID 之间存在不确定窗口时，Runner 保存未知状态并停止重新提交。`agent test` 对异步适配验证提交和一次查询，完整生命周期可用正常 Task 或集成测试验证。

## 调度

Task.schedule 使用且只使用 `at`、`every`、`cron` 中的一种。Cron 为分钟、小时、日期、月份、星期，采用 IANA 时区。日期与星期同时指定时遵循 Cron 库的常规匹配语义。

固定间隔从计划时间推进，不从完成时间推进。夏令时缺失时间跳过，重复时间只取第一次。

`misfire: latest` 为同一 Task 保留最近一次尚未开始的定时 Run，较早的记录显示 skipped。`misfire: skip` 跳过超过正常扫描容差的触发。两种模式都受 `start_grace` 限制。手动执行不参与最近一次定时触发的替换。

默认 `overlap: skip`，starting、running、unknown 都阻止同 Task 新执行。手动触发冲突返回 409。自动执行失败不重试，传输失败会重试。手动重试创建新 Run 并关联原记录。

## 消息与确认

消息版本为 1。任务和控制按设备分主题，事件按设备回传。Runner 仅能访问自己的消费者、确认主题与回复收件箱。

- Gateway：事务创建 Run 和 Outbox → JetStream 持久化确认 → 标记已发布。
- Runner：本地持久化意图 → Gateway 原子领取 → 启动 Agent。
- 输出：本地事件序号与队列 → JetStream 持久化确认 → 本地标记已发布。
- 完成：所有事件成功发布后，确认任务消息。
- 入库：Gateway 去重并写 PostgreSQL 后，确认事件消息。

开始过的同步命令不会因重投重新启动。异步命令保存 job_id 后只恢复查询。RunEvent 使用 `(run_id, seq)` 去重，最终状态不会被迟到的进度覆盖。


Task 的 timezone 未指定时，由 Gateway 按工作区当前默认时区补全并保存。改变工作区默认时区只影响之后创建的任务。Task 编码上限 1000 KiB（为消息封装预留空间）；大型最终结果会转换成带 `truncated: true` 的 64 KiB 预览。

`misfire: skip` 的定时 Run 最多允许 5 秒启动抖动，超过后直接 skipped；设备离线期间的旧执行不会在恢复后补跑。手动执行使用 start_grace，不适用该规则。

Runner 启动时加载 Agent 配置并保存每个 Run 的适配快照。修改本地 YAML 后重启 Runner，新 Run 使用更新后的配置，已提交的异步任务继续使用原快照查询。

## Python HTTP 适配示例

[adapter.py](../examples/adapter.py) 是可修改的标准库 HTTP 包装器。示例服务契约为 POST /jobs 接收 prompt 并返回 job_id，GET /jobs/{id} 返回 wakeit 状态，POST /jobs/{id}/cancel 请求取消。请按实际服务的路由和字段修改它，并将 async-agent.yaml 中的 /opt/my-agent/adapter.py 改成脚本绝对路径。

在 Runner 的运行环境设置 MY_AGENT_SERVICE_URL 与 MY_AGENT_SERVICE_TOKEN，再将 YAML 路径加入 Runner.agents。URL、凭据和完整调用配置留在本机。外部服务需自行实现对 Idempotency-Key 的处理；Runner 仍会对无法确认的提交使用 unknown。

## 参数与即时能力查询

Agent.parameters 声明参数（仅 string、label、default、enum），Task.parameters 保存显式覆盖。解析顺序为 Task > Agent 默认 > 不传参；未知参数和非法枚举会在保存与执行时拒绝。`{{parameters.name}}` 可用于独立参数、stdin、cwd 和 env；未设置的参数应放在 `{when_set: name, args: [...]}` 条件组中。Run.execution_snapshot 保存解析参数、实际 cwd、适配 digest 和模型说明；完整适配快照只留在 Runner journal。动态目录不改变 digest。

Agent.models 是可选 CommandSpec，输出 ModelCatalog：`{version:1,models:[{id,name,parameters,parameter_options?}]}`。条目 ID 不直接传给 Agent，parameters 中的值用于 Task；parameter_options 只描述该模型的选项。没有接口、空结果或错误均不会补充预置/静态/旧模型。目录结果不能保证账号对模型有调用权限。

Runner 在 hello 中上报参数声明、models_supported、trace_supported 和 directory_roots。Core NATS 请求主题为 `wakeit.v1.runner.<uuid>.query`，回复主题为 `wakeit.v1.runner.<uuid>.reply.<request-id>`；请求和回复包含 version、id、runtime_id。Runner 只能订阅自己的 query，发布自己的 reply；Gateway 重新验证设备有效性后返回结果。查询不进入 JetStream 或 journal。

list_directories 的 payload 为 `{path?,cursor?}`，没有 path 返回允许根目录；结果为 `{path,parent,breadcrumbs,directories:[{name,path}],next_cursor}`，每页 200 条。list_models 的 payload 为 `{agent,cwd}`。Runner 先验证真实路径；目录超时 5s，模型查询超时 30s，命令上限 25s。浏览、模型查询和执行均要求配置 allowed_roots，不接受指向范围外的符号链接。

Agent.logo 为可选图片地址：Gateway 根路径（例如 `/agent-logos/codex.svg`）或 HTTPS URL。Runner 将其随名称和能力上报；第三方 Agent 未配置时显示名称首字母，图片加载失败也使用同样的首字母。该字段不读取 Runner 本地图片文件；内置图标随 Gateway 静态资源嵌入。


## 原生会话

Task 默认每次新建独立且持久化的原生会话；绑定现有会话时设置 `session: {id: "完整原生ID"}`。不接受名称、前缀、最近会话或文件路径。更换设备、Agent 或工作目录后需重新绑定。

同步 events 适配可声明 `session`，同时提供 `inspect`、`resume`、`interactive` 三个 CommandSpec。模板变量 `{{session.id}}` 只在已明确选择会话时可用。`inspect` 不使用任务指令，只读返回 `{"version":1,"id":"完整ID","cwd":"原生目录","persisted":true}`。Runner 要求 ID 精确匹配、已持久化、目录真实路径相同；检查失败直接失败，绝不退回普通 submit。交互命令继承终端输入，不能配置 stdin。

适配执行期间输出实际会话元数据：

```json
{"version":1,"type":"session","data":{"version":1,"id":"完整ID","cwd":"/srv/project","persisted":true}}
```

事件声明本身不证明可续聊：Runner 在命令结束后通过 inspect 再确认持久化和目录，保存 Run.session 的 id、runner_id、agent、cwd、ready、reason。失败或取消的执行也可保留有效会话。没有元数据的历史 Run 不追溯猜测会话。

Session 元数据和可靠事件在本地 SQLite 同一事务提交，再通过现有 JetStream 通道回传。元数据不消耗日志/Trace 容量；7天内容清理不会删除它，随90天运行记录保留策略清理。离线补传和事件重投按序号幂等合并，迟到的旧元数据不覆盖已确认的状态。

Gateway 领取执行时对 Runner + Agent + Session 加事务锁，starting/running/unknown 跨 Task 互斥；占用时跳过本次执行。Runner 和 `wakeit run resume` 另共用本地文件锁。unknown 在用户核对结束前保持占用。取消只终止当前执行，不删除会话。绑定任务重试沿用绑定，独立任务重试新建会话。

本地续聊仅使用当前用户登记的 Runner 配置和 Journal 中保存的适配快照，重新验证会话与目录，再执行本地原生交互命令；本地记录完整时无需 Gateway 或 API Key。它不会下载并执行服务器提供的命令。原生 Agent 在其他终端直接启动的执行不受 Wakeit 锁控制。


### 已有会话目录与原生续聊命令

`session.list` 为可选的只读 CommandSpec，在选定目录中执行，返回完整元数据目录：

```json
{"version":1,"sessions":[{"id":"完整原生ID","title":"会话标题","cwd":"/srv/project","updated_at":"2026-10-15T01:00:00Z"}],"next_cursor":null}
```

`title`、`updated_at` 可省略或为null。不返回聊天正文、日志、凭据或会话文件路径。Runner要求目录真实路径与所选cwd一致、ID唯一且完整，再在本机按更新时间和ID倒序排序、搜索和每页20条分页。适配命令需返回完整目录，上限10000条；不完整目录明确报错，不把首段当全量。没有list接口、离线或失败时仍可手填完整ID，不使用静态或旧候选列表。

内置目录查询使用Codex thread/list、OpenCode session.list、Pi项目会话header与Claude项目会话元数据。选择后仍可显式inspect，执行前始终再校验。列表可能过期，目录条目不是执行授权或持久化的最终证明。

Run.session.command保存Runner根据本地interactive快照生成的原生交互命令，包含安全引用的工作目录和独立参数；Windows使用PowerShell，Unix使用POSIX shell。不导出任务指令、stdin和环境变量值；依赖额外env/env_from的自定义交互命令不提供复制入口，仍可使用本地CLI。WebUI复制该原生命令，`wakeit run resume`继续作为带本地校验和互斥的可选入口。直接启动原生Agent不受Wakeit互斥控制。


Kimi Code、Antigravity CLI 和 Gemini CLI 的原生协议分别归一化，不因命令风格相近而共用解析器。AGY 的单次任务先编码为一个 JSON stdin 事件；并发读 stdout/写 stdin，防止大型初始化输出和多行指令互相阻塞。新增适配的真实能力与官方依据见[内置适配](agents.md)。

### Activity recording causes

`trace_status.data` retains `status` and may include `reason`: `content_shortened`, `event_limit`, `event_unsupported`, `event_parse_failed`, `event_too_large`, `content_limit`, or `content_expired`. These describe recording quality, not execution outcomes. Ordinary adapter diagnostic logs do not imply incomplete activity. An adapter can explicitly attach optional `recording_issue` to a version-1 log record; the Runner emits the corresponding status once per cause. The Runner also recognizes explicit truncated input/output previews. Existing logs without that field remain valid. Old Runners can omit an unknown diagnostic record and retain their generic incomplete marker; execution and terminal-result messages are unchanged.

The WebUI displays causes in a toolbar information tooltip for the actual execution. Historical statuses without a cause remain unspecified; the UI does not infer a cause from human log text. No activity warnings are inserted ahead of recorded messages. Empty, loading, unsupported and fully expired activity remain distinct empty states.
