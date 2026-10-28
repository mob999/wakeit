# CLI 指南

`wakeit --help` 和各子命令的 `--help` 是参数的完整来源。Gateway 管理调度和任务，Runner 在设备上执行命令，CLI 通过同一 REST API 管理工作区。

## 地址与认证

```sh
export WAKEIT_GATEWAY=https://wakeit.example.com
export WAKEIT_WORKSPACE=WORKSPACE_ID
export WAKEIT_PASSWORD='你的账号密码'
wakeit login --username your-name
unset WAKEIT_PASSWORD
```

个人 API Key 由连接配置私密保存，跨工作目录生效；也可显式设置 `WAKEIT_TOKEN_FILE` 或 `WAKEIT_TOKEN`。不要把 API Key 写进任务 YAML。

Admin（管理员）管理本区成员、设备与设置；Operator（读写）注册设备、管理任务和运行；Viewer（只读）查看本区数据。全局账号管理需要独立的实例管理员权限。Runner 的独立设备身份不能代替个人 API Key。

API 查询及更新命令支持全局 `--json`。适配导出、进程启动和服务配置生成等本地命令遵循自己的输出形式。

## 定义和注册任务

`task.yaml` 是任务配置，不是 Gateway 的存储。`task apply` 校验配置并提交到 PostgreSQL。只改本地 YAML 不会改变已注册任务。

```yaml
version: 1
id: daily-review
name: 每日仓库检查
runner: dev-server
agent: codex
cwd: /srv/my-project
prompt: 检查仓库，整理需要关注的问题。
parameters: {}
schedule:
  cron: '0 9 * * *'
  timezone: Asia/Shanghai
execution:
  timeout: 1h
  start_grace: 1h
  overlap: skip
  misfire: latest
```

名称、Runner 和目录替换成实际值。模型通过 `parameters.model` 指定，其他参数必须由所选适配声明；留空时采用适配或 Agent 默认值。

```sh
wakeit task apply --file task.yaml
wakeit --json task list
wakeit task get daily-review
wakeit --request-key review-once --json task trigger daily-review
wakeit task pause daily-review
wakeit task resume daily-review
```

`task apply --expect <revision>` 用于基于已读取的版本更新；冲突时先核对其他成员的更改，不强行覆盖。同一逻辑触发重试传输时复用 `--request-key`；用户明确发起的新运行用新的键。

除 Cron 外，`schedule` 可为 `{every: 30m}`、`{at: '2026-12-01T01:00:00Z'}` 或 `null`（只手动执行）。配置 Schema 可由 `wakeit schema task` 导出。执行目录必须在该 Runner 的 allowed_roots 内。

归档停止调度、撤销排队执行，已领取的 Run 继续；历史保留，归档任务不能编辑、触发或重试。恢复后保持暂停，原时间配置保留，只有明确恢复调度时才执行 resume：

```sh
wakeit task archive <task-id>
wakeit --json task list --archived
wakeit task restore <task-id>
wakeit task resume <task-id>
```

WebUI 可在设置 → 归档任务恢复；成员关系和邀请在设置 → 成员管理，需要本区管理员。全局账号新建、启停及本地密码重置通过实例账号管理完成，需要实例管理员。停用或重置密码会使该账号已有登录与 API Key 全部失效。

## DAG 任务

一个 Task 可以通过内嵌 `dag` 定义整张 AND 图，多个根统一开始，不支持绕过依赖独立触发或取消节点。失败节点可在原 Run 内手动重试，保留成功节点。WebUI 提供两步图编辑器，CLI 使用相同 `task apply --file`、trigger 和 run watch 命令。配置、结果引用、输出选择、warning 与取消规则见 [DAG 指南](dag.md)，完整示例见 [task-dag.yaml](../examples/task-dag.yaml)。

## 任务模板

模板将类型化业务参数、环境绑定和普通/DAG 蓝图分开。Gateway 在创建前生成并校验完整 Task，之后不再运行模板引擎；已有 Task 和历史 Run 不随模板改动。完整定义与输入文件示例见[任务模板指南](templates.md)。

```sh
wakeit template validate --file examples/task-template.yaml
wakeit template apply --file examples/task-template.yaml
wakeit --json template list
wakeit template get repository-monitor
wakeit template preview repository-monitor \
  --inputs-file values.yaml --bindings-file bindings.yaml
wakeit --request-key create-review-001 --json task create \
  --template repository-monitor --inputs-file values.yaml \
  --bindings-file bindings.yaml --run
```

`validate` 离线检查定义和语法；`preview` 使用服务端编译器，但不创建 Task 或 Run。`apply --expect <revision>` 保护模板更新，`list --archived` 查询归档模板，`archive` / `restore` 只影响后续实例。`from-task <task-id> [--file <path>]` 将现有 Task 转为新模板定义，保留字面文本和具体上游结果引用，不覆盖已有文件。

`task create --template` 支持 `--inputs-file`、`--bindings-file`、`--schedule-file`、`--id`、`--name`、`--expect` 和 `--run`。输入文件包含 `{values:{...},unset:[...]}`，保留实际 JSON 类型和显式省略；绑定文件按环境槽位提供 Runner、Agent 和适配参数。`--run` 原子创建 Task 和一次 Run，不加时只创建 Task。网络失败后的同一次重试应使用原请求和 `--request-key`，避免产生重复实例。模板定义 Schema 可由 `wakeit schema template` 导出。

## 查看、取消与重试

```sh
wakeit --json run list --task daily-review
wakeit run get <run-id>
wakeit run logs <run-id> --follow
wakeit run cancel <run-id>
wakeit --request-key retry-once run retry <run-id>
wakeit run retry <run-id> --node <node-id>
wakeit run retry <run-id> --node <node-id> --resume-stopped
```

### 阻塞观察一次运行

```sh
wakeit run watch <run-id>
wakeit run watch <run-id> --timeout 1h
wakeit --json run watch <run-id> --timeout 10m
```

`run watch` 只观察指定 Run，实时显示状态、日志和 Agent 实际提供的过程，结束后显示最终结果并退出。已结束的 Run 直接读取已有输出和结果。它不会触发、重试或取消任务，也不会切换到下一次执行；不提供 `task watch`。

默认一直等待。`--timeout` 为大于零的时长，覆盖请求、重连和等待；它不改变任务本身的执行超时。`Ctrl+C` 只退出观察。`unknown` 会提示等待核对，不视为结束。

每秒查询一次，事件按游标补读；终态后补读当前已入库的尾部事件，再读取最终结果。断线后最多重连 30 秒，观察超时仍优先生效。内容截断、过期或未记录时会明确提示；没有结果时不从日志拼接报告。

`--json` 输出 JSONL，每行含 `type`、`run_id`、`data`；事件记录还含 `event_id` 和 `seq`。DAG 子事件还携带 `node_id`、`execution_id` 和 `original_seq`；Trace 按 DAG 节点 ID 与原生 Trace ID 组合归并。类型为 `status`、`log`、`trace`、`notice`、`error`、`summary`。Trace 是按节点 ID 更新的完整快照。最后一行 `summary.data` 包含 `state`、`terminal`、`reason`、`result` 和 `exit_code`；超时或读取错误不会伪造执行终态。输出不包含整份任务配置或无关 Session 信息。

| 退出码 | 含义 |
|---|---|
| `0` | Run 成功，或 DAG 警告（warning） |
| `1` | 失败、取消、执行超时、跳过或中断 |
| `2` | 参数、认证、查询或持续连接错误 |
| `124` | 观察等待超时 |
| `130` | 用户中断观察 |

取消请求发出不等于已取消。`unknown` 不自动重试；先确认原进程或远端任务已经结束，再记录实际状态：

```sh
wakeit run resolve <run-id> --state interrupted --reason '已确认原进程结束'
```

然后才能考虑重试。不带 `--node` 创建新的关联 Run；DAG 节点重试保留原父 Run。

## 绑定会话与本机续聊

支持会话的同步适配默认每个 Run 新建独立、持久化的原生会话。原生标题统一为 `任务名-wakeit-YYYYMMDD-HHMMSSmmm`（毫秒），例如 `每日仓库检查-wakeit-20261015-093012045`；使用工作区时区及实际开始时间，同一 Run 只生成一次。总长不超过 200 个 UTF-8 字节，过长时截短任务名称部分。标题是显示信息，真正绑定和续聊仍使用原生 Session ID。需要反复在同一历史上下文上执行时，给任务添加原生会话 ID：

```yaml
session:
  id: ses_existing_session
```

ID 来自实际 Agent 会话或已有 Run 的 session 信息；不是 Run ID，也不是本地文件路径。绑定的会话必须属于目标 Runner、Agent 和工作目录。更换它们时重新选择会话。没有 session 能力的适配不能绑定，不根据日志猜测会话 ID。会话不存在或不可恢复时明确失败，不静默新建。

WebUI 在任务高级设置中使用单一“会话”选择器，默认“每次新建”。可按标题或 ID 搜索已有会话，选择后自动检查；手填 ID 位于弹层底部。检查失败保留选择并显示错误，离线仍可手填和保存，执行前由 Runner 重新严格检查。

任务每次触发都会继续绑定的会话，历史上下文随执行累积，保留原生标题，不因 Wakeit 任务名不同而改名；未绑定的任务及其重试仍创建独立会话及新的开始时间标题。重试绑定任务会再次向原会话提交指令，不是撤销上一次修改。

完成后，WebUI 的“复制续聊命令”提供该 Agent 的**原生命令**，包含工作目录。例如 Codex 的 POSIX shell 形式：

```sh
cd -- '/srv/my-project' && codex resume '实际会话ID'
```

复制内容已按 Runner 的系统引用参数：macOS/Linux 用 POSIX shell，Windows 用 PowerShell。命令在原 Runner 同一用户的终端执行；它直接启动原生 Agent，不经过 Wakeit 的会话存在检查和互斥锁。先结束其他终端或 Wakeit 中使用同一会话的执行。不要在其他设备上直接运行，也不要把原生命令中的会话 ID 换成“最近一个”。

若希望保留本地检查与 Wakeit 互斥，可选用 CLI 入口：

```sh
wakeit run resume <run-id>
# 无法发现配置时显式指定：
wakeit run resume <run-id> --runner-config /absolute/path/runner.yaml
```

这个入口从本用户登记的 Runner 配置及本地执行记录读取已保存适配，不要求 Gateway 在线或 CLI API Key，也不重新提交 Task。命令检查目录范围、实际会话和占用情况，再启动原生 Agent 的交互界面。执行中、unknown、没有可靠会话 ID 或没有持久化会话的 Run 不能从此入口续聊。原生程序的退出码由 CLI 透传。

原生命令不包含任务指令、环境变量实际值或凭据。适配的交互入口需要额外 env/env_from 时，不导出原生命令；本地 CLI 仍可按保存的适配配置使用该环境。

`runner enroll` 和 `runner start` 自动登记本用户的配置路径。登记表只保存路径，位于 Linux 的 `$XDG_CONFIG_HOME/wakeit`（默认 `~/.config/wakeit`）、macOS 的 `~/Library/Application Support/wakeit`、Windows 的 `%APPDATA%\wakeit`。它不复制登录凭据。命令需要本地 Run 记录和 Agent 会话文件，不能迁移到任意设备执行，也不在本地记录已清理后自动寻找“最近会话”。

同一会话的 Wakeit 执行及 `wakeit run resume` 互斥，占用时新执行跳过；`overlap: allow` 不允许并发写同一会话。unknown 在核对结束前保持占用。直接运行原生 Agent 会绕过 Wakeit 锁，开始续聊前先结束其他终端中的同一会话。

交互续聊会保留本地占用标记，确认原生进程及其子进程全部退出后才释放。CLI 被强制结束或无法确认进程树终止时，后续使用会被阻止。先核对对应 Agent 进程树确已全部停止，再删除错误消息给出的那个占用标记文件；不要批量清理或用修改 Run 状态绕过检查。

## 设备与适配

```sh
wakeit --json runner list
wakeit runner enrollment
wakeit runner enroll --name dev-server --token-env WAKEIT_ENROLL_TOKEN \
  --dir /path/to/runner --allow-root /path/to/projects
wakeit agent doctor --config /path/to/runner/runner.yaml
wakeit runner start --config /path/to/runner/runner.yaml
wakeit runner revoke <runner-id>

wakeit agent init codex --file codex.yaml
wakeit agent validate --file codex.yaml
wakeit agent test --file codex.yaml --cwd /path/to/project
```

注册和吊销需要 Admin。Agent 测试实际调用 Agent；默认指令只要求简短问候，异步测试会提交一个外部任务并查询一次状态。完整的异步恢复和取消需要通过 Runner/Run 验证。

内置适配可在本机只读查询已有会话：`wakeit adapter session list codex --cwd /path/to/project --limit 20`，分页使用返回的 next_cursor 对应 `--cursor`，搜索使用 `--query`。protocol 同样支持 claude-code、pi、opencode、kimi-code；只返回所选工作目录下的会话标题与 ID 等元数据，不提供 query/首条指令预览，不执行任务。

Runner 自启与 Agent 登录身份保持一致，见 [运维指南](operations.md#runner-自启)。第三方适配指南和 fixtures 见 [协议](protocol.md)。

## 检查与更新

```sh
wakeit update --check
wakeit --json update --check
wakeit update
```

GitHub 更新凭据依次读取 `WAKEIT_GITHUB_TOKEN`、`GH_TOKEN`、`GITHUB_TOKEN`，最后尝试 `gh auth token --hostname github.com`。公开 Release 无需登录，配置这些凭据仅用于提高 API 限额。它与 `WAKEIT_TOKEN` 无关。

JSON 返回 `status,current_version,latest_version,update_available,target`：`no_release`、`up_to_date`、`update_available`、`updated` 为正常状态；失败返回 `status: error` 和不含凭据的错误，退出码 1。

只检查正式 SemVer Release，排除草稿和预发布；版本必须高于当前包版本。同版本的提交不会触发更新。没有可用正式版本时返回 no_release；已是最新版本时返回 up_to_date。

更新读取同一 Release 的 `wakeit-release.json`，校验平台、文件大小、SHA-256 和候选程序的 `--version`，才替换当前可执行文件。更新锁阻止并发替换；目录不可写时返回错误，不自动提权。已有 Gateway/Runner 继续运行旧进程，服务升级需另行备份、迁移和重启。

Homebrew、系统包管理器或容器管理的安装应使用相应升级方式。自更新适用于直接安装的二进制；不会同步修改 Docker 镜像或其他位置的 Wakeit。

## 浏览器接入与连接

`wakeit setup --gateway URL --workspace WORKSPACE_ID [--runner-name NAME] [--allow-root PATH]...` 通过浏览器设备授权连接当前 CLI，注册 Runner，安装用户级自启并验证在线。默认名称是主机名，默认范围是当前目录；浏览器显示全部拟执行目录。已有身份、适配和范围优先复用。失败保留完成步骤，再次运行继续，不能用同名设备接管另一身份。

`wakeit login --gateway URL --workspace WORKSPACE_ID` 仅授权 CLI；显式 `--username` 保留密码登录。浏览器打开失败时手动打开输出的验证链接，核对验证码并授权；请求十分钟过期。API Key 默认不过期，按 Gateway、工作区和账号分别私密存储，Settings → API Key 可撤销。

`wakeit connection list` 列出连接名称、地址、工作区、账号及当前连接；`wakeit connection use NAME` 切换。配置位于 macOS `~/Library/Application Support/wakeit`、Linux `$XDG_CONFIG_HOME/wakeit`（默认 `~/.config/wakeit`）、Windows `%APPDATA%/wakeit`。显式参数及 WAKEIT 环境变量优先，地址与凭据严格匹配。不要将凭据写入项目或聊天。

setup 自启名称包含 Runner ID，支持多 Gateway。Linux 使用 systemd user，macOS 使用当前用户 LaunchAgent，Windows 使用当前用户登录计划任务。默认不保证注销后或 Windows 登录前运行；Agent 的登录和模型保持原样。若服务管理器不可用，setup 报告未完成步骤，不改用 root。


内置 Agent 的命令、模型发现、过程与会话能力见[适配指南](agents.md)。模板使用 `wakeit agent init <id> --file <id>.yaml` 导出；安装能力检测使用 `wakeit adapter detect <id>`，不发起推理。新增 ID 包括 `kimi-code`、`agy`、`gemini-cli`、`cursor`、`copilot`、`cline`、`goose`、`qwen-code`。

工作区连接不能跨区复用。多项连接匹配时，先用 `connection use NAME` 明确选择；`--workspace`（或 `WAKEIT_WORKSPACE`）指定作用域。旧连接仅在原 Gateway 认证成功并返回其固定 Default 工作区后补全元数据，不替换凭据或 Runner 配置；失败保留原文件。新 Runner 实例固定一个工作区，同机多工作区使用分别隔离的配置、Journal 和服务。

WebUI 的“续聊”现在直接连接 Runner 上的原生交互终端（管理员/读写），同时保留标题栏复制完整原生命令。只读成员、演示区及旧版 Runner 仍可复制。关闭弹窗结束进程，不删除会话；意外断线最多保留 30 秒用于重连。网页终端与 `wakeit run resume`、Task 共用 Session 互斥和 Runner 容量，不生成新的 Run。直接运行复制出的原生命令仍不受 Wakeit 锁控制。

## DAG node retries

A node may set `max_retries: 3` for the initial attempt plus up to three automatic retries; manual attempts are separate. Omitted/0 disables automatic retries. Confirmed execution failures and node timeouts retry after 5/10/20/40/60 seconds with bounded jitter; validation, invalid Session, cancellation and unknown do not automatically retry. Older Runners without the optional state-event retry verdict conservatively do not retry failed executions. Optional graph total timeout includes backoff and does not extend on retry within an active Run; a settled Run gets a new window when explicitly recovered. Attempts remain child Runs with their own logs, Trace, result and native Session; default sessions start anew, explicitly bound sessions are reused under the same locks.

Admin/Read-write members can retry a confirmed failed node with `run retry ID --node NODE` in the same frozen Run. Successful nodes remain unchanged; dependent skipped nodes resume after AND succeeds. Each manual attempt runs once without resetting or consuming automatic quota. Pending automatic successors are accelerated, not duplicated. `--resume-stopped` explicitly recovers graph cancellation/total timeout after all old processes stop; unknown must be reconciled. Task archival, retained-result availability and overlap are checked again. The CLI reads the latest node and submits its required nullable execution ID. Without `--node`, whole-graph retry still creates a new Run from current Task configuration. See [DAG recovery](dag.md#manual-node-retry-and-recovery).
