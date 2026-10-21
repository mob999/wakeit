# 运行

```sh
wakeit --json run list --task <task-id>
wakeit run get <run-id>
wakeit run watch <run-id> --timeout 1h
wakeit --json run watch <run-id> --timeout 1h
wakeit run logs <run-id> --follow
wakeit run cancel <run-id>
wakeit --request-key <stable-retry-key> run retry <run-id>
wakeit run retry <run-id> --node <node-id>
wakeit run retry <run-id> --node <node-id> --resume-stopped
```

需要等待任务完成时，先从实际触发响应或运行列表取得 Run ID，再使用 `run watch`，不要猜测 ID 或用反复重试代替等待。watch 仅观察这次 Run，实时显示状态、日志和真实 Agent 过程，结束后输出结果；已结束的 Run 直接读取返回。不提供 `task watch`，也不会自动触发下一次执行。

默认无限等待，`--timeout` 必须大于零，限制整个观察过程；`Ctrl+C` 和观察超时均不取消任务。`unknown` 保持等待核对。退出码为成功及 DAG warning 0、其他执行终态 1、参数/认证/查询错误 2、观察超时 124、用户中断 130。

`--json` 返回 JSONL：`type`、`run_id`、`data`，事件另有 `event_id`、`seq`；最后的 `summary.data` 包含实际状态、`terminal`、原因、结果和退出码。超时或网络错误不表示 Run 已结束。Trace 是完整节点快照，按 ID 更新。需要单次读取时仍用 `run get`，仅跟随日志时用 `run logs --follow`。WebUI “执行”页展示实际过程和结果，“详情”页提供日志及运行信息。纯文本适配只有输出，不从文本猜工具调用。

取消操作是请求；确认最终 cancelled 才算完成。终止无法确认、同步监控丢失或异步提交结果不确定会进入 unknown。核对本机进程或外部 job_id 的实际状态，确认后：

```sh
wakeit run resolve <run-id> --state interrupted --reason '实际核对说明'
```

支持的人工最终状态见 `wakeit run resolve --help` 和 API 返回错误，不能仅因离线把运行标记成功或已取消。整轮重试会创建关联的新 Run；DAG 的 `--node` 在原 Run 内补跑所选失败节点并保留成功节点，二者都可能再次产生外部副作用；用户要重试且原执行结束确认后再操作。

输出可能明确截断或过期，报告中保留该状态，不把缺少输出解释为没有执行。默认日志/过程保留 7 天，运行记录 90 天。

用户要继续对话时，读取实际 Run 的 session.command，按 [会话](sessions.md) 提供原生 Agent 续聊命令。需要本地验证和互斥时，可选用 `wakeit run resume <run-id>`。二者均与 `run retry` 不同：续聊在本机打开已有会话，retry 重新提交一次任务；不要相互替代。

DAG Run 代表整图。日志和 Trace 的子事件带 node_id、execution_id、original_seq；按 DAG 节点和原生 Trace ID 组合聚合，不混用不同节点的工具事件。输出 result.outputs 保留所选节点的独立报告。触发、取消和手动整轮重试作用于整图，整轮重试读取当前任务定义并从全部根开始。unknown 须通过父 Run 的 nodes/{node}/resolve 接口逐节点核对，不强制标记父 Run。续聊必须读取具体节点的真实 Session；不得把父 Run ID 当原生会话 ID。

DAG 的运行状态和异常节点菜单位于“执行 / 详情”页签右侧，状态的 Tooltip 展示整轮原因。异常菜单按当前尝试列出失败、超时、中断及待核对节点，跳过节点单独分组；定位会切回当前执行，不把已恢复的旧失败算作异常。节点的日志、过程、报告及续聊必须来自同一实际 execution。整轮重试与配置中的节点自动重试不同；“立即重试”加速同轮已排队的自动尝试；“重试节点”只新增一次手动尝试，不消耗或重置自动额度，失败后也不自动追加。成功后恢复因依赖失败而跳过的下游，继续遵守 AND。取消或总超时需明确 `--resume-stopped`，先确认旧执行全部停止；其他独立失败或取消分支保持原样。归档、unknown、前置未成功、结果不可复用和重叠冲突会阻止恢复。历史尝试不可执行重试。原 Run ID、冻结定义和成功记录保持不变；新 Watch 可观察同一恢复后的 Run。

## Result file previews

Admin and Read/write members can click explicit local links in a saved Run/DAG report or Moments to preview a current Runner file. Viewer can copy paths; demo workspaces never query real files. No separate preview_roots is configured: the exact saved link and current Runner allowed_roots bound access. Relative targets use the execution snapshot's task directory, which may differ from an adapter's overridden process directory; adapters should return absolute links in that case. Current files are not archived historical artifacts.

`POST /runs/{execution-id}/files/preview` accepts `{href, document?}`. DAG uses the actual child execution ID. Gateway verifies authored Markdown/HTML anchors or image sources in the persisted result. For images in a Markdown-file preview, `document` must be an original result link; Gateway rereads that complete Markdown file, verifies its image reference and resolves the image relative to the Runner-validated document directory. Ordinary nested file links, code examples and forged references do not grant access. Direct result-relative paths use the frozen execution cwd. Runner keeps the existing version-1 `read_result_file` query, identities and allowed_roots; no new roots, protocol fields or database schema are required. The existing 512 KiB raw limit (further bounded by NATS) applies to each read. UTF-8 text/code, Markdown/JSON and complete PNG/JPEG/WebP are supported; HTML/SVG remain escaped source. PDF, downloads, directories and special files are unsupported. Admin/Operator reports, Moments and Markdown-file previews load visible local images with at most four concurrent reads, memory-only scoped caching, retry and click-to-enlarge. Viewer/demo performs no local file IO. Closing cancels unneeded reads; permissions are rechecked before delivery. Responses are no-store and image bytes never become Gateway filesystem URLs or browser-persisted content.


## Moments outcome reading

Moments remains the workspace home and a separate completion-time timeline. Each parent Run has a compact report card; DAG outputs switch inside the card. Report outlines and explicit local/external links come from sanitized original content, not AI summaries or filesystem discovery. The context pane only indexes the loaded report; excerpts/truncation are marked. Visible or expanded cards lazily read the existing Run endpoint for full content, elapsed wall time and actual retried-node counts. Results refresh manually and do not share the live-record polling cache. Local files use the original selected node's preview authorization; continuation and copying target that same output. Linked files are current Runner files, not archived or proven newly generated artifacts. No extra API/configuration is required.
