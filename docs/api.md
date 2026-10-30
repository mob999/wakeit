# REST API v1

业务基路径 `/api/v1/workspaces/{workspace-id}`，JSON 请求与响应。账号登录、工作区目录、实例账号管理、OAuth 与设备执行通道保留全局 `/api/v1` 基路径。错误为 `{"error":"说明"}`。需要登录的接口支持 `Authorization: Bearer <token>`；WebUI 使用 HttpOnly 会话 Cookie。


工作区由路径显式确定，API Key 固定一个工作区；浏览器会话只代表全局账号，业务角色来自目标区成员关系。不存在或不可访问的工作区/资源返回 404。旧的无工作区业务地址返回 426 和 `workspace_required`，不能通过请求头或其他标签页的选择改变作用域。

全局接口：`GET/POST /api/v1/workspaces` 列出已加入的工作区及演示入口 / 创建并成为 Admin；`GET/PUT /api/v1/workspaces/{workspace-id}` 读取 / 更名；`POST .../leave` 退出。成员接口为 `GET .../members?page=1&page_size=20`、`PUT/DELETE .../members/{user-id}`，修改 `{role?,active?}` 或移除成员。最后有效 Admin 不能退出、停用或降权。

`/api/v1/session` 返回全局身份、`instance_admin`、凭据类型及 API Key 的固定 `workspace_id`；全局身份中的兼容 `role` 不用于业务授权。`/api/v1/instance/users` 及其账号启停、密码接口仅接受实例管理员的浏览器会话，不授予工作区数据读取权。

Cookie 写请求需要 `X-Wakeit-Request: 1`，浏览器 Origin 必须匹配 Gateway.public_url。API Key 请求不依赖 Cookie。权限变更和 API Key 撤销立即从共享数据库生效。

| 接口 | 请求或结果 | 权限 |
|---|---|---|
| GET `/changelog?lang=zh\|en` | `{version,content}`，随 Gateway 交付的 Markdown 更新日志；默认英文，其他语言返回 400 | 公开 |
| POST `/session` | `{username,password}`，设置 Cookie 并返回会话 token | 未登录 |
| GET / DELETE `/session` | 当前用户 / 退出 | Viewer |
| GET `/tasks` | 支持 page、page_size、q、runner、agent、enabled；含 last_run | Viewer |
| GET `/tasks/{id}` | 单个 Task | Viewer |
| POST `/tasks/validate` | `{config}`，只读完整校验，返回 `{errors}` | Operator |
| GET `/task-templates` | 模板分页、搜索及归档筛选 | Viewer |
| GET / PUT `/task-templates/{id}` | 模板定义和 revision / 保存完整定义 | Viewer / Operator |
| POST `/task-templates/{id}/archive`、`/restore` | `{expected_revision}`，归档或恢复模板 | Operator |
| POST `/task-template-preview` | `{definition,request}`，只读预览未保存草稿 | Viewer |
| POST `/task-templates/{id}/preview` | InstanceRequest，只读生成最终配置、图和 source map | Viewer |
| POST `/task-templates/{id}/instantiate` | InstanceRequest + Idempotency-Key，原子创建 Task 及可选 Run | Operator |
| GET `/tasks/{id}/template` | 将现有 Task 转为新模板定义，保留文本和结果引用 | Viewer |
| PUT `/tasks/{id}` | `{config,expected_revision}`；新建版本为 null，更新须匹配 | Operator |
| POST `/tasks/{id}/archive`、`/tasks/{id}/restore` | `{expected_revision}`，归档或恢复为暂停 | Operator |
| POST `/tasks/{id}/runs` | 返回 `{id}` | Operator |
| POST `/schedule/preview` | Schedule 对象，返回接下来五次 UTC 时间 | Viewer |
| GET `/runs` | 支持 page、page_size、q、runner、agent、task、state、before | Viewer |
| GET `/runs/{id}` | Run、状态、结果、配置快照 | Viewer |
| GET `/runs/{id}/nodes/{node}` | `{node,execution}`，含冻结节点配置与真实执行 | Viewer |
| GET `/runs/{id}/nodes/{node}/events?after=0` | 节点事件，最多 500 条 | Viewer |
| POST `/runs/{id}/nodes/{node}/resolve` | 核对具体 unknown 节点，父状态自动聚合 | Operator |
| GET `/runs/{id}/events?after=0` | 按游标读取，最多 500 个事件 | Viewer |
| GET `/runs/{id}/stream?after=0` | SSE，事件名 run_event，支持 Last-Event-ID | Viewer |
| POST `/runs/{id}/cancel` | 保存取消请求；确认完成后才进入 cancelled | Operator |
| POST `/runs/{id}/retry` | 创建关联的新 Run | Operator |
| POST `/runs/{id}/resolve` | `{state,reason}`；仅 unknown，记录核对说明 | Operator |
| GET `/runners`、`/agents` | 设备及其公开适配能力 | Viewer |
| POST `/runners/enrollment` | 一次性设备注册 token，兑换时复核签发人的本区权限 | Operator |
| POST `/runners/{id}/revoke` | 撤销设备；报告 broker_revocation_pending | Admin |
| GET `/members` | 工作区成员；支持 page/page_size，分页结果含 active_admins | Admin |
| POST `/api/v1/instance/users` | `{username,password}` 创建全局本地账号，不自动加入工作区 | 实例管理员 |
| PUT `/members/{id}` | `{role?,active?}`，省略字段保持当前值；禁止移除最后一名有效管理员 | Admin |
| POST `/api/v1/instance/users/{id}/password` | `{password}` 重置本地密码，撤销全局登录与 API Key | 实例管理员 |
| POST `/invites` | `{role}`，返回 token 和邀请链接 | Admin |
| POST `/api/v1/invites/accept` | 已登录账号 `{token}`；有效邀请允许本地新账号提交 `{token,username,password}` | 邀请持有人 |
| GET / POST `/tokens` | API Key 元数据支持 page/page_size / `{name,expires_at?}` 创建，secret 仅返回一次 | Viewer |
| DELETE `/tokens/{id}` | 撤销本人 API Key | Viewer |
| GET / PUT `/settings` | 时区与日志/运行保留天数；写需要 Admin | Viewer / Admin |
| GET `/audit` | 最近 100 个审计记录 | Admin |

触发和重试要求 `Idempotency-Key`，同一用户、操作目标和键返回相同 Run ID。不同键且存在冲突执行时返回 409。编辑版本冲突也返回 409。

设备使用独立 Token，不能调用用户 API。设备接口为 `/device/enroll`、`/device/hello`、`/device/heartbeat`、`/device/claim`，由 Runner 自动处理。设备注册要求一次性令牌及用户 NKey 公钥，私钥由设备生成并保存。

健康检查：`/health/live`、`/health/ready`。Prometheus 格式指标：`/metrics`，需要实例管理员会话，包含调度延迟、Outbox 积压、设备离线和各 Run 状态数量。

动态 OpenAPI 文档位于 `/api/v1/openapi.json`。可用 `wakeit schema openapi`、`wakeit schema task`、`wakeit schema template`、`wakeit schema agent` 导出定义。

## 任务模板

InstanceRequest 包含 `expected_revision`、`values`、`unset`、命名 `bindings`，以及可选 `task_id`、字面 `name`、Schedule 覆盖、`run_now` 和预览 `request_id`。预览返回 `resolved_inputs`、`config`、`graph`、`source_map`、`used_bindings` 和 `issues`；有错误时 `config` 可为空，已生成的图仍可用于定位问题。预览不写 Task、Run、事件或 Outbox。

实例化重新在服务端编译并检查 revision、归档状态、权限和绑定。成功返回 `{task_id,task_revision,run_id,template_revision}`；`run_now:false` 时 `run_id` 为 null，否则为新建 Run UUID。模板、Task 和 Run 保留创建时的来源快照，后续模板编辑或归档不会重新生成已有配置。

实例化要求 `Idempotency-Key`。同一工作区、用户和键的相同已提交请求返回原 Task/Run ID，即使模板之后已修改或归档；重放前仍复核权限。同键不同请求、过期的新 revision 或冲突的 Task ID 返回 409。校验失败返回 422 和 `{error:"taskTemplate.validation",issues:[...]}`，issue 包含 code、逻辑 path、message，以及可用的 node_id、line、column，不产生部分写入。完整格式和示例见[任务模板指南](templates.md)。

## 即时能力 API

- `GET /api/v1/meta`：公开返回 Gateway 版本（如 `version:"0.4.3"`）、公开地址和登录方式。该版本不代表 CLI/Runner 版本。
- `GET /api/v1/workspaces/{workspace-id}/runners/{id}/directories?path=...&cursor=...`：Admin/Operator 浏览 Runner 目录；无 path 时列根目录。
- `POST /api/v1/workspaces/{workspace-id}/runners/{id}/agents/{agent}/models/query`：Admin/Operator 查询模型，请求 `{cwd:"绝对路径"}`，返回 ModelCatalog。

查询错误分别返回 400（目录/适配错误）、403（角色限制）、404（设备不存在或已吊销）、503（离线）、504（超时）。成功但无模型的响应是空数组。CLI 与 WebUI 保存 Task.parameters，Run.execution_snapshot 提供最终解析参数和工作目录。

## 列表分页与搜索

WebUI 使用 `GET /tasks?page=1&page_size=20` 与 `GET /runs?page=1&page_size=20`。传入 page 时返回 `{items,total,page,page_size}`，page 从 1 开始，page_size 默认为 20、最大为 100。总数与本页数据在同一条 PostgreSQL 查询中读取；排序增加唯一 ID 作为同时间记录的次序。持续新增或编辑记录时，翻页会反映当前数据，而非固定历史快照。

q 在名称、ID、指令、工作目录、Runner、Agent 字段上进行不区分大小写的部分匹配；空格分隔的词按顺序匹配，百分号与下划线作为字面字符处理。runner 和 agent 按名称/ID 精确筛选；任务可用 enabled 筛选启用/暂停，运行可用 state 筛选状态。所有读取均保留 Viewer 权限要求。未传 page 的调用返回数组，运行最多 100 条；CLI 与脚本可显式请求分页。

全局搜索的 Runner、Agent 使用公开目录进行匹配；Settings 搜索只显示当前权限可见的设置项；所有角色可查看工作区设置，仅 Admin 可修改配置或管理成员。搜索不读取个人 Token 的值。

## 设置页接口

`GET /timezones` 需 Viewer 身份，返回 Gateway 内置时区库实际支持的名称数组，按名称排序并去重。设置页通过该目录搜索选择时区，目录请求失败显示重试，保留现有选值。

`GET /members?page=1&page_size=20` 返回 `{items,total,page,page_size,active_admins}`，成员包含 id、username、role、active、created_at，按 created_at 和 id 升序排列。active_admins 是整个工作区有效管理员总数。角色修改仍由 Gateway 原子保护最后一名管理员；前端提交时保留成员已有 active 值。

`GET /tokens?page=1&page_size=20` 返回 `{items,total,page,page_size}`，仅包含当前用户的 API Key 元数据：id、name、created_at、expires_at，按创建时间和 id 倒序排列。查询不返回密钥。上述接口不传 page 时返回数组；分页规则与 Tasks/Runs 相同。创建 API Key 名称去除首尾空白且不能为空。

创建 API Key 时 `expires_at` 可选，省略或为 `null` 表示不过期；指定值须为未来的 RFC 3339 时间。列表中的 `expires_at: null` 同样表示不过期。已有 Key 的到期时间保持不变；浏览器登录会话仍为 7 天。

WebUI 将邀请链接和新 API Key 密钥限制在创建弹窗内，关闭或离开后清除；创建结果不进入 Query 缓存、URL、本地存储、日志或通知。工作区编辑有离开保护，成员及 API Key 操作独立刷新各自查询。

## Agent 过程

Runner 的 Agent 清单提供 trace_supported。Run.execution_snapshot 保存执行时的能力、解析参数和目录，历史记录不依赖当前 Runner 的适配配置。旧运行没有该能力快照时显示“此运行未记录过程”，不会从日志推断 Trace。

`GET /runs/{id}/events?kind=trace&after=0` 仅返回 trace 与 trace_status，按事件 id 升序，每批最多 500 条。继续使用最后一条 id 作为 after，直到返回不足 500 条；它与日志缓冲区独立。不给 kind 则返回全部事件，其他 kind 值按事件种类精确筛选。SSE 仍发送全部事件，支持 Last-Event-ID 跨 Gateway 重连，前端按节点稳定 id 和首次序号聚合，迟到的进行中快照不覆盖完成节点。权限均为 Viewer。

事件行包含 id、run_id、seq、kind、data、created_at；trace 的 data 遵循 StreamRecord.data 的 TraceNode Schema，内容可用状态为 trace_status 的 status: incomplete、truncated、expired。Run 状态事件职责保持不变，没有新增 Trace 表或调度接口。

## 成员账号与任务归档

成员新建复用现有用户名和密码校验。停用或重置密码，在同一事务中撤销全部会话与 API Key；启用账号不恢复旧凭据。角色和状态可独立修改，省略的字段保留当前值。操作自身账号同样适用；修改自身密码后需要重新登录。审计仅记录操作者、目标和操作种类，不包含密码。首位管理员仍通过 gateway bootstrap 初始化。

Task 返回可空 archived_at；GET /tasks 默认排除归档任务，archived=true 仅返回归档任务，支持原有分页与筛选，要求 Operator。GET /tasks/{id} 仍可查看归档任务。归档和恢复通过 expected_revision 检查版本，冲突为 409；重复同状态操作不递增版本和审计。归档撤销所有 queued Run，starting/running 继续，unknown 不变；禁止编辑、触发与重试。恢复保持暂停、next_at 为空，重新启用后从当前时间计算，不补跑归档期间任务。Task ID 始终保留。

Run 列表和详情增加 task_archived，反映所属任务的当前归档状态；执行快照不变，历史仍遵循保留策略。归档任务的取消、核对状态与内容读取权限不变。

## Moments 结果时间线

WebUI 的 `/` 为 Moments，任务工作台为 `/tasks`。Moments 仅手动刷新，以工作区时区和完成时间展示所有来源的已结束 Run；归档任务的历史仍可见。没有完成时间，以及 queued、starting、running、unknown 状态的 Run 不进入动态。

- `GET /moments/days?before=YYYY-MM-DD&limit=14`：返回 `{timezone,today,total_runs,days:[{date,count}],next_before}`。默认 14 个有记录的日期，日期倒序，before 为排他的本地日期游标。总数覆盖所有符合条件的记录，不受分页影响。
- `GET /moments?date=YYYY-MM-DD&cursor=...&limit=20&timezone=...`：返回 `{timezone,date,items,next_cursor}`，按 finished_at、id 倒序分页。timezone 可携带日期目录返回值；工作区时区变化时返回 409，需刷新目录重新分组。两个接口 limit 范围 1–100，权限为 Viewer。
- 条目包含 id、task_id、name、state、reason、finished_at、runner、agent、logo、preview、format、has_more、truncated，不返回指令或整份配置。preview 最多 2048 个 Unicode 字符；format 为 markdown 或 json，has_more 表示仍有正文，truncated 表示原始结果已被保留策略截断。全文通过既有 `/runs/{id}` 读取，不从日志生成。动态页对可见/主动展开的卡片懒取详情，用于真实历时及节点尝试事实；报告目录和文件入口从清理后的原文提取，不新增接口或文件扫描。DAG 在一张父 Run 卡片内切换实际输出。

日期只做分组，所有条目统一展示最高 240px 的预览，长内容提供“阅读全文”和“收起”。日期和条目均支持继续加载；手动刷新保留阅读位置及全文展开状态，返回页面保留本次访问状态，刷新浏览器后重置。报告不写入浏览器持久化存储。


## 会话检查与续聊

`POST /runners/{id}/agents/{agent}/sessions/inspect` 接收 `{cwd,id}`，要求 Admin 或 Operator，Core NATS 查询超时30秒。响应为 `{version:1,id,cwd,persisted:true}`；不存在、不持久化、目录不符、设备离线或适配查询失败时返回明确错误。检查不会创建 Task、Run 或会话。Task 保存不要求设备在线，执行前始终重新检查。

Agent 目录增加 `session_supported`；Task.config 增加可选 `session: {id}`。Run 列表及详情增加可空 `session`：`{id,runner_id,agent,cwd,ready,reason}`，来自实际执行元数据，不修改配置快照。Moments 摘要增加 `continuation_available`。所有角色可读取并复制续聊命令，创建绑定任务仍要求 Operator。会话可用性并不保证当前未被其他执行占用，本地续聊和 Runner 启动时均重新校验。


`POST /runners/{id}/agents/{agent}/sessions/query` 接收 `{cwd,q?,cursor?}`，Admin/Operator可用，超时30秒。返回 `{version:1,sessions:[{id,title,cwd,updated_at}],next_cursor}`，每页20条。搜索匹配标题及完整ID，游标沿用同一目录和查询条件。Runner Agent清单增加 `session_list_supported`，不支持目录发现时只提供手填。

Run.session增加可空command，为Runner执行时生成的原生Agent续聊命令（包含目录切换），WebUI不自行推测命令。Moments.continuation_available仅在会话可用且存在该命令时为true。未记录会话的历史Run保持不变；已确认的会话可由Runner从本地适配快照补充原生命令，不重新执行Agent。

## Device authorization

GitHub login is configured per Gateway. `GET /api/v1/auth/github/start?return_to=/…` starts an authorization-code flow with S256 PKCE and a short-lived browser-binding cookie. `GET /api/v1/auth/github/callback` consumes the state once, verifies the immutable GitHub account ID and establishes a global Wakeit browser session. These routes request no repository/email scope and never return GitHub tokens. `/meta` exposes `auth.mode` and `auth.allow_registration`, never credentials. See [Workspace deployment](workspaces.md) for configuration and demo isolation.

`GET /api/v1/meta` adds `public_url` and `install` (explicit official release, manifest, checksums and package base URL). No secrets are included. The topbar Agent Prompt uses this configured origin.

Device-flow endpoints accept JSON. `POST /oauth/device/code` accepts `{workspace_id,name,os,register_runner,allowed_roots}` and returns `device_code,user_code,verification_uri,verification_uri_complete,expires_in:600,interval:5`. `POST /oauth/token` accepts `{grant_type:"urn:ietf:params:oauth:grant-type:device_code",device_code}`. Errors include `authorization_pending`, `slow_down` (increase interval by five seconds), `access_denied`, `expired_token`, `invalid_grant`. A successful exchange returns `access_token,token_type,user` once; loss before local persistence requires reauthorization.

`GET /device-authorizations/{user_code}` and `POST .../approve|deny` require a Web session cookie; API keys and bearer sessions cannot approve. Cookie writes require existing CSRF headers/origin checks. Merely loading a URL never approves. Requests and rate limits live in PostgreSQL, codes are hashed, and responses use no-store. Approval and exchange recheck active account and role.

Admin/Operator may register; Viewer may authorize CLI-only. Revocation remains Admin-only. `POST /workspaces/{workspace-id}/runners/connect` accepts `request_id,public_key,name,allowed_roots,timestamp,signature` and requires Operator plus Ed25519/NKey proof over canonical JSON produced by `device_auth::proof`. Persist request ID and private key before calling. Retrying the same signed identity reissues the device bearer and invalidates the prior bearer; different keys, owners, revoked devices and name conflicts are rejected. OAuth-issued setup keys cannot change the approved name or roots.

## AND DAG

完整配置、状态和同轮结果引用见 [DAG 指南](dag.md)。普通 Task 形态保留；DAG 与顶层执行字段互斥。所有任务写入使用同一完整校验及 revision 检查，不分节点提交。Run 新增 nodes；selected result 为 `{type:"dag",outputs:[{node_id,name,state,reason,result}]}`。终态新增 warning；unknown 继续非终态。

Task、Run、搜索、历史与 Moments 只计入整图，不把子执行列为普通 Run。结果预览 format 可为 dag，outputs 按节点分段且共用 2048 Unicode 字符预算。runner、agent 在整图摘要中可空，不代表任意单个节点。父事件以 ID 为连续游标，子事件含 node_id、execution_id、original_seq。触发、重试、取消仅作用于整图；子执行的独立写入口由服务端拒绝。

Task GET 返回独立的 `editor_layout`；PUT 接受可选 `{version:1,positions:{node_id:{x,y}}}`，与配置和 revision 原子保存。省略或 null 保留仍存在节点的坐标，删除节点时清理；普通 Task 不接受 DAG 布局。布局不进入 YAML、Run 快照或 Runner 消息。`POST /tasks/validate` 只读校验完整配置和可选布局，最终 PUT 再次校验。

## Interactive session terminals

Workspace Admin and Operator may create a browser terminal for an ended Run with an actual, resumable session. Viewer and demo access remains command-copy only. These endpoints never modify the source Run, result, or Moments.

| Endpoint (workspace base) | Behavior |
| --- | --- |
| `POST /runs/{id}/terminals` | `{request_id,node_id?}`; idempotent lifecycle reservation. A DAG node resolves to its actual execution. No process starts yet. |
| `GET /terminals/{id}` | Owner credential's lifecycle metadata: `id,state,generation,reason,exit_code`. |
| `POST /terminals/{id}/attach` | Returns `{ticket,expires_in:30}`; single-use, credential/workspace-bound. |
| `GET /terminals/{id}/stream` | Same-origin WebSocket. First text frame within 5 seconds: `{ticket,cols,rows}`. Never put the ticket in the URL. Starts only after this authenticated frame. |
| `POST /terminals/{id}/close` | Idempotent process-tree stop; waits for confirmation. Unconfirmed cleanup retains Session occupancy. |

Client frames are `{kind,data}`: `input` with `{seq,data:base64}`, `resize` with `{cols,rows}`, `snapshot`, and `heartbeat` every 5 seconds. Input sequence starts at 1 per attachment and is never replayed. Server frames are `snapshot`, `output`, `state`, `ack`, and `error`. A snapshot carries bounded ANSI screen/input-mode state and output sequence; live frames additionally identify workspace, Runner runtime, terminal, generation, and sequence. Output gaps cause a new snapshot rather than replaying input.

Only one writer is admitted. Unexpected disconnection retains the PTY for at most 30 seconds; reconnect uses the existing process. Explicit close, node switch, logout/revocation or workspace departure stops it. A changed Runner runtime never restarts an old terminal. The Runner loads its local execution snapshot, validates the exact native Session and real directory, and invokes the frozen interactive adapter. Browsers cannot provide command strings. Terminal bytes are not stored in Run events, PostgreSQL, SQLite, or browser storage; lifecycle metadata and private local process identity are persisted separately.

## DAG node retries

A node may set `max_retries: 3` for the initial attempt plus up to three automatic retries; manual attempts are separate. Omitted/0 disables automatic retries. Confirmed execution failures and node timeouts retry after 5/10/20/40/60 seconds with bounded jitter; validation, invalid Session, cancellation and unknown do not automatically retry. Older Runners without the optional state-event retry verdict conservatively do not retry failed executions. An optional graph total timeout includes backoff and does not extend during active retry; explicit recovery of a settled Run starts a new window at the first claim. Attempts remain child Runs with their own logs, Trace, result and native Session; default sessions start anew, explicitly bound sessions are reused under the same locks.

The parent/node retry endpoint now accepts a required nullable `execution_id` and optional `resume_stopped:false`, with `Idempotency-Key`. It returns `{id,parent_run_id,node_id}`. Node detail exposes retry action, blocking reason and affected descendants. One manual attempt retains successes within the frozen parent, does not consume/reset automatic quota, and does not schedule automatic follow-ups on failure. Pending automatic successors are reused. Explicit stopped recovery requires old processes confirmed stopped and no unknowns; only affected descendants resume under AND. Successful retained results must remain reusable. Archive, permission and overlap guards run transactionally. DAG total timeout is disabled when omitted/null; existing explicit limits remain unchanged. See [DAG recovery](dag.md#manual-node-retry-and-recovery). Migration 0007 adds attempt origins, structured stop causes, recovery generation/window and idempotent request identities; back up before migrating. It preserves existing Run IDs, attempts and devices.

## Result file previews

Admin and Read/write members can click explicit local links in a saved Run/DAG report or Moments to preview a current Runner file. Viewer can copy paths; demo workspaces never query real files. No separate preview_roots is configured: the exact saved link and current Runner allowed_roots bound access. Relative targets use the execution snapshot's task directory, which may differ from an adapter's overridden process directory; adapters should return absolute links in that case. Current files are not archived historical artifacts.

`POST /runs/{execution-id}/files/preview` accepts `{href, document?, image?, offset?, revision?}`. DAG uses the actual child execution ID. Gateway checks the exact saved result link on every request; Markdown-file images additionally require an original linked, complete Markdown document and a current image reference inside it. Relative paths use the frozen execution cwd or Runner-validated Markdown directory. Runner uses the existing version-1 `read_result_file` query and current allowed_roots; no separate roots, database changes, or protocol-version change is required. Optional image fields are additive.

Text/Markdown/JSON retain the 512 KiB raw preview limit, further bounded by NATS. Inline PNG/JPEG/WebP uses `image: "thumbnail"`: Runner decodes a source of at most 32 MiB / 40 megapixels / 16,384 pixels per side and generates a preview of at most 1280 pixels per side, reducing it further to fit one response. Transparency is preserved; thumbnail encoding may differ from the original. At most two thumbnails decode concurrently per Runner. Clicking opens a preview and reads the unchanged original using `image: "original"`, byte `offset` and the returned `revision`; each response contains `offset`, `next_offset` (null at EOF), base64 content and the original total `size`. The first original chunk establishes the current file revision (a cached thumbnail may be older); later chunks must match it. All chunks are independently authorized, including current Markdown references and permissions. Original data is memory-only and discarded on close; closing cancels further reads. Old Runners retain small complete-image compatibility and show an update message for oversized files.

Reports, Moments and Markdown-file previews lazily load visible thumbnails with at most four concurrent browser reads, scoped memory-only caching and click-to-enlarge. Inline failures show their specific cause as well as Toasts. Original failures retain the thumbnail and display the cause. Viewer/demo performs no local file IO. Responses are no-store/nosniff. No files are rewritten, no image bytes are persisted to a Gateway URL or browser storage, and no image is substituted for the original report result. HTML/SVG remain escaped source; PDF, directory browsing, special files and downloads are unsupported.

Activity `trace_status` events can carry the structured `data.reason` described in [the protocol](protocol.md#activity-recording-causes). Unknown or absent reasons remain unspecified. Recording notices are scoped to the actual execution and do not change the Run state.
