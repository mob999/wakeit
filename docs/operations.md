# 部署与运维

## 原生部署

先构建 WebUI 和 `wakeit`。准备 PostgreSQL，初始化专用 NATS 身份目录：

```sh
wakeit gateway nats-init --dir ./data/nats
nats-server --config ./data/nats/nats.conf
```

默认只监听本机，初始化生成操作员、工作区和系统身份及持久化目录。多 Gateway 共用同一套身份文件，避免每个实例重新生成身份。

Gateway 配置示例：

```yaml
version: 1
listen: 127.0.0.1:8080
public_url: http://127.0.0.1:8080
database_url_env: WAKEIT_DATABASE_URL
nats_url: nats://127.0.0.1:4222
nats_public_url: ws://127.0.0.1:8081
nats_auth_dir: ./data/nats
nats_replicas: 1
log_limit_bytes: 20971520
```

身份目录的相对路径以配置文件目录为准。数据库 URL 只从指定环境变量读取。

```sh
export WAKEIT_DATABASE_URL='postgres://wakeit:password@127.0.0.1/wakeit'
wakeit gateway migrate --config gateway.yaml
export WAKEIT_PASSWORD='your-admin-password'
wakeit gateway bootstrap --config gateway.yaml --username admin
unset WAKEIT_PASSWORD
wakeit gateway start --config gateway.yaml
```

环境覆盖：`WAKEIT_PUBLIC_URL`、`WAKEIT_NATS_URL`、`WAKEIT_NATS_PUBLIC_URL`、`WAKEIT_NATS_REPLICAS`。NATS URL 可用逗号分隔多个节点。首版创建 Stream 时设置副本数；已有部署调整副本数前，须通过 NATS 管理工具迁移三个 WAKEIT Stream 的副本配置。对外 URL 应与浏览器访问域名保持一致。

## 云端多实例

使用 `deploy/compose.ha.yaml`：两个 Gateway、三个 NATS 节点、JetStream 三副本、Caddy HTTPS/WSS 入口。PostgreSQL 由外部服务提供高可用。

```sh
export WAKEIT_DATABASE_URL='你的外部 PostgreSQL URL'
export WAKEIT_WEB_HOST='wakeit.example.com'
export WAKEIT_MQ_HOST='mq.example.com'
docker compose -f deploy/compose.ha.yaml --profile tools build init
docker compose -f deploy/compose.ha.yaml --profile tools run --rm init
docker compose -f deploy/compose.ha.yaml --profile tools run --rm migrate
export WAKEIT_PASSWORD='your-admin-password'
docker compose -f deploy/compose.ha.yaml run --rm -e WAKEIT_PASSWORD gateway-1 \
  gateway bootstrap --config /app/gateway.yaml --username admin
unset WAKEIT_PASSWORD
docker compose -f deploy/compose.ha.yaml up -d
```

为两个域名配置 DNS，入口开放 80/443。NATS 客户端和集群端口保留在私有网络。HA 示例的节点仍位于同一宿主机；跨故障域部署时，将同一身份配置分发到三个独立节点，各节点保留自己的持久化目录。PostgreSQL、NATS 仲裁和入口代理的可用性共同决定整体可用性。

## Gateway 开机自启

Gateway 使用系统级服务，三平台均可在用户未登录时启动。通过 `wakeit gateway service` 生成配置及安装脚本，详细步骤见 [Gateway 系统服务](gateway-service.md)。数据库与 NATS 同样需要系统自启，不能仅设置 Gateway。

## Runner 自启

使用 Agent 已登录的用户运行 Runner。先在该用户环境下完成 `agent doctor` 和试跑，再导出本机自启配置：

```sh
wakeit runner service --config /absolute/path/runner.yaml --platform linux --output wakeit-runner.service
wakeit runner service --config /absolute/path/runner.yaml --platform macos --output dev.wakeit.runner.plist
wakeit runner service --config C:\wakeit\runner.yaml --platform windows --output install-runner.ps1
```

Linux：将配置放入该用户的 `~/.config/systemd/user/`，执行 `systemctl --user enable --now wakeit-runner`。需要退出登录后继续运行时，管理员为该用户开启 linger。

macOS：放入该用户 `~/Library/LaunchAgents/`，通过 `launchctl bootstrap gui/$(id -u) <plist路径>` 加载。

Windows：在该用户 PowerShell 中运行生成的脚本，创建登录触发的计划任务。需要登录前执行时，使用专用服务账户设置“无论用户是否登录都运行”，并为该账户配置 Agent 认证、目录权限和环境。

后台服务的 PATH 可能与交互式终端不同。Agent 配置中使用绝对可执行文件路径可消除差异；Windows 的 npm Node 入口会自动解析为 `node` 和脚本路径，其他脚本通过配置显式指定解释器。设备休眠或关机时不能执行任务，恢复后遵循 Task 的补跑与有效期设置。

## 备份与恢复

备份包括三部分：PostgreSQL、NATS 持久化数据、NATS 身份目录。Runner 的本地记录和凭据保留在设备上。

采用一致的停机备份流程：暂停任务，等待正在执行的任务结束或完成核对；停止 Runner、Gateway 和 NATS；对 PostgreSQL 执行 `pg_dump -Fc`，复制 NATS 数据及身份目录；再启动服务并恢复任务。

```sh
pg_dump -Fc "$WAKEIT_DATABASE_URL" > wakeit.dump
```

Compose 部署可在服务干净停止后备份对应命名卷。身份目录包含签名种子和系统凭据，应与数据库备份一同受控保存。

恢复时先恢复数据库，再恢复同一套 NATS 身份及数据目录，运行 `gateway migrate`，启动 NATS 与 Gateway，最后连接原 Runner。不要重新运行 nats-init 生成新身份。外部异步任务可能仍在执行，恢复时通过已保存的 job_id 核对；本地同步执行仍按 unknown 语义处理。

## CLI 更新

`wakeit update --check` 检查正式版本，`wakeit update` 校验并替换当前二进制，GitHub 私有凭据与 Gateway Token 独立。没有正式发布时返回 no_release；保持 0.1.0 的提交不触发更新。详细行为见 [CLI 指南](cli.md#检查与更新)。

## 升级与监控

构建新镜像，备份后单独运行迁移，再滚动替换 Gateway。保持协议 v1 兼容后替换 Runner；不要删除它的 data_dir。收到 SIGTERM 时服务停止接收新工作；Runner 记录未完成异步任务，后续恢复查询。

监控 `/health/ready` 和带 Admin Token 的 `/metrics`。重点关注调度延迟、Outbox 积压、Runner 离线、unknown 与 failed 的增长。NATS 存储达到配额时拒绝新消息，Runner 保留未上传结果，恢复容量后补传。

默认运行记录 90 天、日志 7 天，可在 Settings 调整。Gateway 持久化数据都在 PostgreSQL，多个实例无需共享日志文件或依赖会话粘性。


吊销 Runner 会暂停它关联的 Task，取消尚未开始的 Run，已启动但无法再核对的 Run 进入 unknown。重新分配设备后再恢复 Task。

## 未发布版本的本机运行

版本固定 0.1.0，数据库迁移只有一份完整初始迁移；升级此预览结构需要明确重建测试数据库，不提供旧结构升级路径。重建前检查活跃 Run，备份 PostgreSQL、设备配置与 journal，并保留 Agent 自身凭据。

正式本机预览使用独立的 runtime 目录与用户服务管理（macOS launchd），PostgreSQL、NATS、Gateway 和 Runner 独立托管。集成脚本仅运行隔离的验收栈，不用它承载日常预览。测试栈和 runtime 都处于 .local/，不得提交凭据或运行数据。

## WebUI 热重载

开发时可以由 Vite 提供 WebUI，Gateway 保持独立运行。Gateway 的 `listen` 指向后端端口，`public_url` 指向浏览器实际访问的 Vite 地址；Runner 的 Gateway 地址同样指向此公开地址。

例如 Gateway 监听 `127.0.0.1:8080`，`public_url` 为 `http://127.0.0.1:5173`：

```sh
cd web
npm ci
WAKEIT_DEV_GATEWAY=http://127.0.0.1:8080 \
WAKEIT_DEV_PUBLIC_URL=http://127.0.0.1:5173 \
  npm run dev -- --host 127.0.0.1 --port 5173 --strictPort
```

Vite 将 `/api` 和 `/health` 代理到 Gateway，包括会话、写操作和 SSE 实时日志；修改 React 或 CSS 后会自动更新浏览器。开发服务器可由用户服务管理器独立托管，避免依赖临时测试进程。生产部署仍使用嵌入 Gateway 的静态 WebUI。

## 未发布归档字段升级

全新数据库直接执行唯一的 0001_initial.sql。已部署的归档功能前 0.1.0 数据库，先验证隔离环境，短暂停止所有 Gateway，保留 Runner 运行；使用当前环境中的 WAKEIT_DATABASE_URL 执行：

```sh
python3 scripts/upgrade-unreleased-archive.py --backup .local/backups/archive-upgrade.dump
wakeit gateway migrate --config gateway.yaml
```

可通过 --pg-bin 指定 PostgreSQL 工具目录。脚本要求已知旧初始迁移校验和，先生成权限受限的 pg_dump 自定义格式备份，再事务内添加 archived_at、更新调度索引和初始迁移基线。未知基线拒绝执行，普通 migrate 继续校验文件完整性；重复升级不重复改动。随后使用新二进制启动 Gateway 并验证 ready。无需重建数据或重新注册设备；不要删除 SQLx 迁移记录或账号。备份保留在忽略目录，恢复时使用 PostgreSQL 的 pg_restore 工具并按照正常维护流程进行。


## 未发布会话结构升级

从归档功能完成后的0.1.0数据库升级到会话版本，先停止 Gateway 写入并确认没有 starting/running Run，保留 Runner 和 Agent 凭据。设置 `WAKEIT_DATABASE_URL` 后执行：

```bash
python3 scripts/upgrade-unreleased-session.py --backup /private/backups/wakeit-before-sessions.dump
wakeit gateway migrate --config gateway.yaml
```

脚本仅接受已知上一版迁移校验值，先创建0600的完整备份，再在事务内增加会话列和索引、同步唯一初始迁移基线；未知结构拒绝执行。已是目标基线时只验证并退出。更早的归档前基线使用 `upgrade-unreleased-archive.py`，它一次升级到当前完整结构。SQLite Journal 在 Runner 启动时原地增加元数据列，保留已有执行记录。

## Unreleased device-authorization baseline

For an existing known pre-device baseline, stop Gateway writers, preserve Runner processes, and explicitly run `WAKEIT_DATABASE_URL=... python3 scripts/upgrade-unreleased-device-auth.py --backup /private/path/backup.dump`. It creates a private backup, checks the exact prior SQLx checksum, adds grant/limit/registration tables in one transaction and updates the single initial migration checksum. Unknown baselines are refused. Normal migration validation remains enabled. Older archive/session upgrade scripts also create the final device-authorization schema.

The browser authorization origin and copied Prompt use `public_url`; set it to an address reachable from the Agent's device. Production requires HTTPS, including the device verification page. Publish the five native packages and SHA-256 manifest under the explicit v0.1.0 prerelease. Regular update checks exclude preview releases; bootstrap intentionally selects the explicit preview package.
