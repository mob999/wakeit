# 定义与注册任务

收集任务名称、指令、目标 Runner、Agent、设备上的 cwd，以及执行时间/时区。用户已提供的信息无需重复问。查 `wakeit --json runner list` 与 `wakeit --json agent list`，用实际设备名称、适配 ID 和声明参数。

目录是 Runner 上的路径，不是 Gateway 所在机器的路径。没有允许根目录时先配置 Runner；已有任务使用原 ID。新 YAML 的 ID 由 Agent 生成：英文名称作基础，中文转拼音，附加随机短后缀，不要求用户手填 ID。

从本 skill 的 `assets/task.yaml` 复制并修改。parameters 保存适配声明的覆盖值，未指定模型时保持空，使用 Agent 默认配置。时间可为五段 Cron + IANA 时区、every 间隔、带时区的 at 时间，或 schedule: null 仅手动执行。不要把一次性本地时间误当 UTC。

```sh
wakeit --json task apply --file task.yaml
wakeit task get <task-id>
wakeit --request-key <stable-operation-key> --json task trigger <task-id>
wakeit task pause <task-id>
wakeit task resume <task-id>
```

apply 才写入 Gateway；只生成 YAML 不算完成注册。记录返回 revision。编辑任务先 get，保留 ID、enabled/暂停状态，再 `task apply --expect <revision>`；冲突读取最新配置与用户修改合并，不直接覆盖。Schema 可由 `wakeit schema task` 导出。

生成和注册定时任务不默认立即执行。用户要求试跑时才 trigger，随后按返回 Run ID 读取实际结果；同一触发请求因网络失败重试传输时复用 request-key。

默认每天 09:00，使用工作区默认时区（初始 Asia/Shanghai），用户指定的时区优先。CLI 尚无设置查询子命令，可通过已认证的 GET /api/v1/workspaces/{workspace-id}/settings 读取 timezone；无法读取时询问用户，不从其他任务的时区猜测。任务超时 1h、延迟 1h、overlap: skip、misfire: latest。自动调度在 Gateway，Runner 仅执行。

## 归档与恢复

用户要移除任务时使用归档：wakeit task archive <task-id>。默认 task list 不包含归档任务，用 wakeit --json task list --archived 查询。归档停止调度、撤销排队 Run，已领取执行继续，历史保留，ID 不释放。归档任务不能编辑、触发或重试；不要用 apply 隐式恢复。

用户需要恢复时，执行 wakeit task restore <task-id>；它恢复为暂停状态，保留时间配置，不补跑归档期间任务。仅在用户明确要求恢复调度时再执行 task resume。WebUI 在 Settings → 归档任务提供同样操作，要求 Admin 或 Operator。

## 绑定已有会话

用户明确要求复用原会话时，按 [会话](sessions.md) 将 `session: {id: ...}` 写入任务；默认不填，每次执行保持独立。不把 Session ID 当 parameters，也不将已有 Run ID 当 Session ID。每次定时触发都会追加到原生会话，先确认这符合用户意图。

## AND DAG

需要多根、分支或汇合时读取 [DAG 指南](dag.md)。一份 YAML 内嵌整张图，任务 ID 和 schedule 属于整图。不要把节点注册为独立 Task 或用普通任务互相触发来模拟依赖。
