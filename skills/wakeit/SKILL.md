---
name: wakeit
license: Apache-2.0
description: Connect to or deploy Wakeit Gateway, register Runners, define scheduled and AND DAG Agent tasks, inspect and watch Runs, and develop command adapters. Use for Wakeit setup and operation, not generic reminders.
---

# Wakeit

Wakeit 集中调度 Agent 任务，在用户设备上的 Runner 执行。使用单一 `wakeit` 二进制，当前产品版本为 0.3.1，配置和事件协议为 version: 1。

先确认用户当前目标及已有环境：是否有可用 CLI、Gateway 地址、认证、Runner 和执行目录。用户要使用 Wakeit，但未说明服务来源时，问是**连接已有 Gateway**还是**自行部署**；已有明确地址或部署意图时直接采用，不重复问。

按目标只读取相应指南，路径以本 skill 所在目录为基准：

- 连接现有服务或安装 CLI：[连接](references/connect.md)。
- 自己托管服务：[部署](references/deploy.md)，再读取该系统的部署指南。完成后验证系统开机自启及 Gateway 就绪。
- 连接执行设备、选择可用 Agent：[Runner](references/runner.md)。
- 管理成员账号、启停或重置密码：[账号](references/accounts.md)。
- 定义完整 AND 图、多根执行及上游结果引用：[DAG](references/dag.md)。
- 定义、注册或修改任务：[任务](references/tasks.md)。
- 查结果、设备文件预览、日志、阻塞观察执行、取消、核对或重试：[运行](references/runs.md)。
- 任务绑定已有会话、网页终端及本机续聊：[会话](references/sessions.md)。
- 接入新 Agent 或远端服务：[适配器](references/adapters.md)。
- 检查版本和更新：[更新](references/updates.md)。

Gateway 的 PostgreSQL 是 Task 的权威存储；本地 YAML 必须 apply 才生效。Runner 本地保留 Agent 凭据及调用配置。任务目录必须在 Runner 的 allowed_roots 中，空范围不开放执行。

优先使用 CLI 的 `--json` 读取实际对象及能力。没有 models 接口时手填；查询失败不编造模型列表。任务与运行 ID 从返回数据读取，用户只需表达任务名称和意图。

连接已有服务优先 `wakeit setup --gateway URL --workspace WORKSPACE_ID`，浏览器明确授权后保存原生连接并注册 Runner、自启和验证；仅 CLI 使用 `wakeit login --gateway URL --workspace WORKSPACE_ID`。已有连接优先复用。认证也可通过已有环境变量或凭据文件；不要要求用户把密钥贴进对话，也不要打印凭据。部署新服务与连接已有服务分开，不重建现有数据库或 Agent 身份。任务处于 unknown 时先核对原执行，再考虑重试。DAG 失败节点用 `run retry RUN --node NODE` 保留成功节点；恢复整图停止需明确 `--resume-stopped`，不把节点重试换成整轮重跑。
