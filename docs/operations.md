# 部署与运维

## Docker Compose 部署

安装 Docker Compose v2，克隆仓库后在仓库根目录运行：

```sh
git clone https://github.com/mob999/wakeit.git
cd wakeit
cp .env.example .env
# 将 .env 中的 WAKEIT_POSTGRES_PASSWORD 改为仅含字母和数字的随机密码。
docker compose build
docker compose up -d
export WAKEIT_PASSWORD='replace-with-a-generated-admin-password'
docker compose run --rm -e WAKEIT_PASSWORD gateway \
  bootstrap --config /app/gateway.yaml --username admin
unset WAKEIT_PASSWORD
```

打开 <http://localhost:8080> 登录。PostgreSQL、NATS 数据和设备身份保存在命名卷中；更新时保留这些卷。Linux 需启用 Docker 系统自启；macOS、Windows 的 Docker Desktop 登录启动不能代替无人登录时的系统启动。

跨设备使用时，将 Gateway 的 `public_url` 和 NATS WSS 地址配置为设备可访问的地址；公网部署使用 HTTPS/WSS。

## 原生部署

先通过 [Gateway 安装脚本或源码](gateway-service.md#安装程序)安装 `wakeit-gateway`。准备 PostgreSQL，初始化专用 NATS 身份目录：

```sh
wakeit-gateway nats-init --dir ./data/nats
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
wakeit-gateway migrate --config gateway.yaml
export WAKEIT_PASSWORD='your-admin-password'
wakeit-gateway bootstrap --config gateway.yaml --username admin
unset WAKEIT_PASSWORD
wakeit-gateway start --config gateway.yaml
```

环境覆盖：`WAKEIT_PUBLIC_URL`、`WAKEIT_NATS_URL`、`WAKEIT_NATS_PUBLIC_URL`、`WAKEIT_NATS_REPLICAS`。NATS URL 可用逗号分隔多个节点。创建 Stream 时设置副本数；已有部署调整副本数前，须通过 NATS 管理工具迁移三个 WAKEIT Stream 的副本配置。对外 URL 应与浏览器访问域名保持一致。

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
  bootstrap --config /app/gateway.yaml --username admin
unset WAKEIT_PASSWORD
docker compose -f deploy/compose.ha.yaml up -d
```

为两个域名配置 DNS，入口开放 80/443。NATS 客户端和集群端口保留在私有网络。HA 示例的节点仍位于同一宿主机；跨故障域部署时，将同一身份配置分发到三个独立节点，各节点保留自己的持久化目录。PostgreSQL、NATS 仲裁和入口代理的可用性共同决定整体可用性。

## Gateway 开机自启

Gateway 使用系统级服务，三平台均可在用户未登录时启动。通过 `wakeit-gateway service` 生成配置及安装脚本，详细步骤见 [Gateway 系统服务](gateway-service.md)。数据库与 NATS 同样需要系统自启，不能仅设置 Gateway。

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

恢复时先恢复数据库，再恢复同一套 NATS 身份及数据目录，运行 `wakeit-gateway migrate`，启动 NATS 与 Gateway，最后连接原 Runner。不要重新运行 nats-init 生成新身份。外部异步任务可能仍在执行，恢复时通过已保存的 job_id 核对；本地同步执行仍按 unknown 语义处理。

## CLI 更新

`wakeit update --check` 只检查 CLI 的 `cli-v*` 正式版本，`wakeit update` 校验并替换当前二进制，GitHub 私有凭据与 Gateway Token 独立。详细行为见 [CLI 指南](cli.md#检查与更新)。

## 升级与监控

Gateway 使用部署脚本、源码构建或新镜像更新；CLI/Runner 单独更新，不比较二者的产品版本号。服务数据保留在原目录，数据库更新使用 `wakeit-gateway migrate`。Runner 重启时保留 data_dir。收到 SIGTERM 时服务停止接收新工作；Runner 记录未完成异步任务，后续恢复查询。

监控 `/health/ready` 和带 Admin Token 的 `/metrics`。重点关注调度延迟、Outbox 积压、`wakeit_outbox_quarantined` 超限隔离数量、Runner 离线、unknown 与 failed 的增长。NATS 存储达到配额时拒绝新消息，Runner 保留未上传结果，恢复容量后补传。

默认运行记录 90 天、日志 7 天，可在 Settings 调整。Gateway 持久化数据都在 PostgreSQL，多个实例无需共享日志文件或依赖会话粘性。


吊销 Runner 会暂停它关联的 Task，取消尚未开始的 Run，已启动但无法再核对的 Run 进入 unknown。重新分配设备后再恢复 Task。

## 从旧版本升级

正式发布的数据库通过追加迁移升级，不重建数据库、不重新注册设备。升级前备份 PostgreSQL、NATS 数据与身份目录，保留 Runner 配置、journal 和 Agent 凭据。停止所有旧 Gateway 写入，使用新版本运行 `wakeit-gateway migrate --config gateway.yaml`，再启动同版本的所有 Gateway，并确认 `/health/ready` 正常。

从 0.3.x 及更早版本升级时，服务端程序改为独立的 `wakeit-gateway`。将服务配置中的 `wakeit gateway start ...` 改为 `wakeit-gateway start ...`，同时保留原配置与数据路径。CLI/Runner 使用自己的版本线，不要求与 Gateway 版本号相同。

从 0.1.x 升级的资源归入 Default 工作区；实例管理员需显式授权，参见[工作区升级指南](workspaces.md#upgrading-an-existing-service)。已有任务、运行记录和设备身份随正式迁移保留。未发布开发版曾改写初始迁移的数据库不在升级支持范围内，不要修改迁移校验和绕过错误。

## Browser session terminals

浏览器终端需要更新后的 Runner。反向代理须转发 `/api/v1/workspaces/*/terminals/*/stream` 的 WebSocket Upgrade，`public_url` 与浏览器访问来源一致；多 Gateway 无需粘性会话。不记录终端输入或 WebSocket 内容。

关闭终端会停止对应进程树；连接意外断开后由 Runner 的租约回收。终端占用执行槽位，与任务共享原生会话锁。无法确认清理完成时，先核对进程状态再处理，不能直接删除本地占用记录。终端交互和原生工具使用 Runner 用户的权限。

## DAG node retries

节点可设置有限次数的自动重试；手动重试可保留成功节点并恢复受影响的下游。遇到 unknown 先核对实际执行状态。具体策略、重试入口和恢复条件见 [DAG 任务](dag.md#manual-node-retry-and-recovery)。

## Result file previews

管理员和读写成员可打开报告中的本地文件链接，由原 Runner 读取其允许目录内的当前文件；这些文件不是执行时的历史快照。Runner 离线、目录不再允许或文件已移除时，预览不可用。只读成员可复制路径。

支持文本、Markdown、JSON 和 PNG/JPEG/WebP 图片；图片先显示缩略图，点击后加载原图。相对路径以执行目录或 Markdown 文件目录解析。接口字段和大小限制见 [REST API](api.md)。

## WebUI 热重载

开发预览使用 Vite，设置方式见[开发指南](../CONTRIBUTING.md#webui-热重载)。
