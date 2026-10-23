# 更新

```sh
wakeit --json update --check
wakeit update
```

GitHub 凭据与 Gateway Token 分开。依次读取 WAKEIT_GITHUB_TOKEN、GH_TOKEN、GITHUB_TOKEN 或已有 `gh auth token --hostname github.com`。公开 Release 无需 GitHub 登录；普通更新忽略预览版，首次接入使用 Gateway 元数据明确指定的官方版本。

没有可用正式版本时返回 no_release，已是最新版本时返回 up_to_date。草稿、预发布和同版本提交不触发更新；只按正式 SemVer 比较。

先检查，用户要求更新时才执行替换。更新校验 Release manifest、当前编译目标、SHA-256 和候选二进制版本；只替换当前实际执行路径，不更新 Docker 镜像或另一个 PATH 中的副本。目录不可写则说明该安装应由所有者或包管理器更新，不自行提权。

运行中的 Gateway/Runner 不会自动重启。服务升级另行备份、执行独立迁移、重启并验证 ready；保留数据库、Runner journal、NATS 身份和 Agent 凭据。容器/系统包安装用对应更新方式。

从 0.2.0 升级至 0.3.0 时，先备份并停止全部 Gateway 写入，再执行 `wakeit gateway migrate --config gateway.yaml`，统一升级并启动 Gateway，验证 ready 后恢复使用。新增 DAG 与布局迁移保留旧任务、历史和 Runner 身份；旧 Runner 继续接收单次执行的 version 1 消息，不需为 DAG 理解整图。不要混跑新旧 Gateway，也不要改写已发布迁移。

从 0.3.0 升级至 0.3.1 时，备份 PostgreSQL 并保留 NATS、Runner 配置、Journal 和 Agent 凭据；停止所有 Gateway 写入，用新二进制执行 `wakeit gateway migrate --config gateway.yaml`，应用 `0005_terminals.sql` 与 `0006_dag_retries.sql` 后统一启动并验证 ready。升级 Runner 才能启用网页终端、结果文件预览及完整的失败重试判断；SQLite 自动兼容升级，不重新注册设备或改动 Agent 登录。配置和消息协议仍为 version 1。

从 0.3.1 升级到 0.3.2 时，备份 PostgreSQL、NATS 状态和 Runner Journal，停止全部 Gateway 写入后，用新二进制执行独立迁移，应用 `0007_dag_manual_retry.sql`，再统一启动并验证 ready。保留原 Run、成功尝试、设备身份和 Agent 凭据，不重新注册。更新 Runner 以启用缩略图、分块原图及不会中断健康 Agent 的过程记录处理；已有设备继续兼容 version 1。旧 DAG 的显式总时限保持，新 DAG 默认不启用整图总时限。

从 0.3.2 升级到 0.3.3 不需要新增数据库迁移，保留已发布的 `0007_dag_manual_retry.sql` 基线。备份服务数据，替换 Gateway／CLI 二进制并重启 Gateway，加载新版 WebUI 和工作区锁修复后验证 ready。保留 PostgreSQL、NATS、Runner Journal、设备身份、原生会话和 Agent 凭据；已有 Runner 继续兼容，不重新注册设备。配置和消息协议仍为 version 1。
