# 更新

CLI/Runner 和 Gateway 分别更新，产品版本不需要相同。当前 CLI/Runner 为 0.1.0，Gateway 为 0.4.0。

## CLI 与 Runner

```sh
wakeit --json update --check
wakeit update
```

更新只选择 `cli-v*` 正式版本，使用 `wakeit-cli-release.json` 和校验列表验证下载，再替换当前 `wakeit`。保留连接、Runner 身份和 Agent 凭据；运行中的 Runner 需单独重启。公开发布无需 GitHub 登录。没有可用正式版本返回 no_release，已是最新返回 up_to_date。

## Gateway

使用 `deploy-gateway.sh`、Compose 或源码构建更新 `wakeit-gateway`；`wakeit update` 不更新它。先停止写入服务，按需运行 `wakeit-gateway migrate`，再启动 Gateway 并确认 `/health/ready`。保留原数据库、NATS 身份与配置。

从原一体化程序切换时，服务配置必须改为独立程序路径和 `wakeit-gateway start ...`。没有 `wakeit gateway` 兼容命令，也不能用数字大小比较原产品 0.3.x 与新 CLI 0.1.x。CLI 使用安装脚本重新安装；Gateway 的版本线继续递增。

## 历史升级记录

以下记录保留 0.1.x–0.3.x 当时的命令，当前安装入口见上文。

从 0.2.0 升级至 0.3.0 时，先备份并停止全部 Gateway 写入，再执行 `wakeit gateway migrate --config gateway.yaml`，统一升级并启动 Gateway，验证 ready 后恢复使用。新增 DAG 与布局迁移保留旧任务、历史和 Runner 身份；旧 Runner 继续接收单次执行的 version 1 消息，不需为 DAG 理解整图。不要混跑新旧 Gateway，也不要改写已发布迁移。

从 0.3.0 升级至 0.3.1 时，备份 PostgreSQL 并保留 NATS、Runner 配置、Journal 和 Agent 凭据；停止所有 Gateway 写入，用新二进制执行 `wakeit gateway migrate --config gateway.yaml`，应用 `0005_terminals.sql` 与 `0006_dag_retries.sql` 后统一启动并验证 ready。升级 Runner 才能启用网页终端、结果文件预览及完整的失败重试判断；SQLite 自动兼容升级，不重新注册设备或改动 Agent 登录。配置和消息协议仍为 version 1。

从 0.3.1 升级到 0.3.2 时，备份 PostgreSQL、NATS 状态和 Runner Journal，停止全部 Gateway 写入后，用新二进制执行独立迁移，应用 `0007_dag_manual_retry.sql`，再统一启动并验证 ready。保留原 Run、成功尝试、设备身份和 Agent 凭据，不重新注册。更新 Runner 以启用缩略图、分块原图及不会中断健康 Agent 的过程记录处理；已有设备继续兼容 version 1。旧 DAG 的显式总时限保持，新 DAG 默认不启用整图总时限。

从 0.3.2 升级到 0.3.3 不需要新增数据库迁移，保留已发布的 `0007_dag_manual_retry.sql` 基线。备份服务数据，替换 Gateway／CLI 二进制并重启 Gateway，加载新版 WebUI 和工作区锁修复后验证 ready。保留 PostgreSQL、NATS、Runner Journal、设备身份、原生会话和 Agent 凭据；已有 Runner 继续兼容，不重新注册设备。配置和消息协议仍为 version 1。

从 0.3.3 升级到 0.3.4 时，备份服务数据，停止所有 Gateway 写入后，用 0.3.4 执行 `wakeit gateway migrate --config gateway.yaml`，应用 `0008_task_templates.sql`，再统一启动 Gateway 并验证 ready。任务预设、创建来源与幂等记录使用新表和字段；已有任务和运行记录保持不变。结构化 DAG 指令仍渲染成 version 1 执行字符串，现有 Runner 身份、原生会话和 Agent 凭据继续保留。不要混用旧 Gateway 写入结构化指令配置。
