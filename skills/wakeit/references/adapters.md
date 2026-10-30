# 开发适配器

先确认本机 Agent 是同步命令还是异步服务，以及它实际支持的参数和输出。复杂 HTTP/SDK 协议可用 Python/Node 包装，Runner 核心无需修改。

从 `assets/sync-agent.yaml` 或 `assets/async-agent.yaml` 开始。异步 HTTP 包装示例为 assets/async-adapter.py；可直接试跑的无副作用同步事件示例为 assets/events-agent.yaml 和 assets/events-adapter.py，复制到本机后把 args 改为实际脚本绝对路径。固定 version: 1，命令由 command + 独立 args + 可选 stdin 组成，不拼 shell。

```sh
wakeit agent validate --file agent.yaml
wakeit agent test --file agent.yaml --cwd /allowed/project
```

测试会真实调用命令。将适配 YAML 加到 runner.yaml 的 agents 路径列表后重新启动 Runner，上报成功才能由任务选择。验证 sync/async 时使用临时数据和无外部副作用的模拟命令。

模板变量：`task.prompt`、`task.cwd`、`task.id`、`run.id`、`job.id`、`parameters.<name>`、`session.id`（仅已绑定或恢复会话时）、`session.title`（仅执行已领取且已生成新会话标题时）。env_from 的映射值是 Runner 环境变量名称；秘密不写 YAML。子进程收到 WAKEIT_RUN_ID 和 WAKEIT_TASK_ID。

参数声明支持字符串、label、default、enum。Task 的 parameters 覆盖默认值；未设置时条件组省略：

```yaml
parameters:
  model: {type: string, label: 模型}
submit:
  command: /path/to/agent
  args:
    - run
    - when_set: model
      args: [--model, '{{parameters.model}}']
  stdin: '{{task.prompt}}'
```

可选 models 命令在选定 cwd 中执行，输出：

```json
{"version":1,"models":[{"id":"provider/model","name":"Model","parameters":{"model":"model","provider":"provider"}}]}
```

parameters 必须与适配声明对应。没接口只手填，空列表或失败不使用旧目录和静态别名兜底。若提供 logo，使用小尺寸清晰的官方矢量资源，不把 PNG 照片或网站缩略图当 Logo。

同步输出 text、json、jsonl 或 events。普通 stdout 保留最多 1 MiB，JSON/JSONL 会持续排空并完整校验语法；有效的大型结果返回带 `truncated: true` 的 64 KiB 预览，不因预览容量改变实际进程成功状态。截断预览不应用 `result_pointer`，不能作为必需的 DAG 上游结果；应将大型产物写入工作目录并在完整报告中引用文件。原生 stream 辅助入口先在独立的 64 MiB 单帧解析预算内提取 Session 和终态，再限制规范化 Trace/结果正文；传输事件仍限 1000 KiB。events 为统一 JSONL：

```json
{"version":1,"type":"trace","data":{"id":"step-1","kind":"command","status":"running","title":"检查仓库","input":"git status","occurred_at":"2026-10-14T01:00:00Z"}}
{"version":1,"type":"trace","data":{"id":"step-1","kind":"command","status":"succeeded","title":"检查仓库","input":"git status","output":"clean","occurred_at":"2026-10-14T01:00:01Z"}}
{"version":1,"type":"result","data":"## 检查结果\n仓库状态正常。"}
```

同一节点 id 更新用完整快照；parent_id 可表示真实父子关系。不要提供未公开的推理内容。最终结果独立输出，退出码仍决定同步执行成功/失败。可运行 `wakeit schema stream` 获取精确契约，`wakeit plugin exec git:github.com/mob999/wakeit adapter -- stream <protocol> -- <command> <args...>` 归一化内置原生协议。

展示容量不足不能伪造 Agent 执行失败。Copilot、Cursor、Qwen Code 的过程预算耗尽时仅截断或省略过程，继续消费原生终态和最终结果；通过 diagnostic 日志标记过程不完整。最终结果的截断使用 `{"truncated":true,"preview":"..."}`，不得把省略说明当作完整正文供 DAG 下游引用。执行错误、缺少终态及真实协议错误仍需如实报告。

异步 submit 输出 `{"job_id":"..."}`，status 输出 `{"state":"running","message":"..."}` 或最终 `{"state":"succeeded","result":"..."}`。状态支持 queued/running/succeeded/failed/cancelled，命令失败只是查询失败。可选 cancel 后由 status 确认 cancelled。建议外部 API 用 run.id 作幂等键；提交可能成功但取不到 job_id 时进入 unknown，不重新提交。

异步 `agent test` 只提交并查询一次；完整轮询、重启恢复和取消需通过隔离 Runner/Run 检查。Windows 脚本入口显式指定解释器，路径/中文/空格通过独立参数传递。

## 可选会话能力

同步适配可以声明 `session.inspect`、`session.resume`、`session.interactive` 三个 CommandSpec。inspect 必须只查询原生会话是否已存在，不能推理或创建；resume 用于非交互追加任务，interactive 用于本机交互续聊。使用 `{{session.id}}` 独立传参。会话缺失时失败，不回退普通 submit 或使用 last/continue 一类隐含选择。

inspect 的 stdout 是 `{"version":1,"id":"原生ID","cwd":"原生工作目录","persisted":true,"title":"真实原生标题"}`。新建和恢复的事件流均输出 `{"version":1,"type":"session","data":{"version":1,"id":"原生ID","cwd":"原生工作目录","persisted":true}}`，以实际协议为准，可导出 `wakeit schema stream`。不要从日志正则或当前目录最新文件猜 ID；session 事件独立于最终 result。

inspect 不接收任务指令、环境变量实际值或凭据文件内容。文件型会话由适配器在自己的会话存储内解析 ID，不能把任意用户路径当作会话输入。完整使用边界见 [会话](sessions.md)。

可选 `session.list` 命令只读返回 `{"version":1,"sessions":[{"id":"...","title":"...","cwd":"...","updated_at":"..."}],"next_cursor":null}`，供工作目录内的已有会话选择。内置辅助入口是 `wakeit plugin exec git:github.com/mob999/wakeit adapter -- session list <protocol> --cwd <directory> --limit 20`，可用 `--cursor`、`--query`；查询不提交指令。详细字段和分页约定以当前 Schema 为准。

WebUI 的续聊复制来自 interactive 命令的原生 argv 与 cwd，不包含 task.prompt、env 或 env_from 的值。交互入口不要直接写入凭据；需要额外环境时不导出原生命令，可选用本地 `wakeit run resume`。原生命令绕过 Wakeit 互斥，保留这一使用边界。

新建原生会话时，在 submit 的独立参数中使用 `{{session.title}}`，让 Agent 保存真实标题；绑定已有会话的 resume 不传新标题。标题是执行元数据，不添加到 Task 或 parameters。时间由 Gateway 的实际领取开始及工作区时区确定，同一 Run 不重算。

不能在 submit 命名的适配可选配 `session.rename: CommandSpec`，以 `{{session.id}}` 和 `{{session.title}}` 在执行后更新原生名称；仅对新会话调用。rename 后读取 inspect，title 必须与期望完全一致。不支持或保存失败时保留实际信息与错误，不伪造成功标题、不重跑 Agent。Codex 内置通过原生辅助命令 rename，其他内置在 submit 传入名称。


内置模板 ID 为 `codex`、`claude-code`、`pi`、`opencode`、`kimi-code`、`agy`、`gemini-cli`、`cursor`、`copilot`、`cline`、`goose`、`qwen-code`。新增适配可用 `wakeit plugin exec git:github.com/mob999/wakeit adapter -- detect <id>` 只读检查原生帮助/版本。Kimi Code 2.x 支持只读会话索引、精确绑定和续聊，模型手填；agy 支持原生模型查询，但未声明 Wakeit 会话能力。Cursor、Cline、Goose、Qwen Code 提供公开过程及最终结果，Copilot 提供原生最终文本；这些适配的模型手填，不声明未经验证的模型和会话目录。Cursor 默认不强制应用文件修改，Copilot 不自动授予工具权限，Cline 一次性模式沿用原生自动批准默认值，Goose 沿用原生无人值守模式。配置事件输出不意味着支持模型或会话发现，不增加静态模型目录兜底。任务选择器仅展示 Runner 检测到的 Agent；Runner Agents 页仍展示完整安装状态。

普通 `log.diagnostic` 只写日志。明确损失过程记录时，可给同一 version-1 log 添加可选 `recording_issue`（`content_shortened` / `event_limit` / `event_unsupported` / `event_parse_failed` / `event_too_large` / `content_limit`）；Runner 按原因去重生成 `trace_status`，不改变执行结果。内容明确为截断预览时，Runner 也会记录 `content_shortened`。未知历史原因不能由日志文案推测。

插件管理：`wakeit plugin install <Git URL或本地目录>`，用 `list` 查看、`update [来源]` 更新、`remove <来源>` 移除；Runner 下次启动生效。包根清单 `wakeit-plugin.json` 声明 version 1 协议和 Agent YAML；命令相对包根目录解析，参数中的包内文件可用 `{{plugin.root}}`。官方适配器预装，第三方也可只写脚本并输出现有 events 协议。
