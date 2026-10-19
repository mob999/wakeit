# 部署 Gateway

先查现有服务和用户目标：本机测试还是常驻服务、地址/域名、系统与账号、已有 PostgreSQL/NATS。如果是连接已有 Gateway，转 connect.md，不在当前机器创建数据库。先安装 CLI，见 connect.md。

Gateway 自己不执行模型推理；Runner 使用独立的 Agent 已登录身份。新安装包含 Default 工作区，可继续创建独立工作区。本地只绑定回环，公开部署需要 HTTPS 和可达的 NATS WSS/TLS 地址。

## 选择部署路径

Linux 的 Docker Compose 适合单机整套托管：有仓库访问权限时获取源码，修改 .env 中的数据库密码，执行 docker compose build、up -d，再通过 gateway bootstrap 初始化管理员。确认 Docker 的系统服务 enabled，且 PostgreSQL/NATS/Gateway 都是 restart: unless-stopped。

需要无人登录的三平台原生常驻服务时读取且只读取对应系统：

- [Linux](deploy-linux.md)：系统级 systemd。
- [macOS](deploy-macos.md)：系统级 LaunchDaemon。
- [Windows](deploy-windows.md)：WinSW 系统服务。

Docker Desktop 在用户登录后启动不是系统开机自启；macOS/Windows 不用它宣称无人登录部署已完成。

## 公共初始化

在系统持久目录保存本 skill 的 assets/gateway.yaml，更新实际端口和对外地址，保留 PostgreSQL URL 在单独的受限环境文件：

```dotenv
WAKEIT_DATABASE_URL='postgres://wakeit:URI_SAFE_PASSWORD@127.0.0.1:5432/wakeit'
```

密码由本机生成并保存，不输出到聊天。创建专用数据库角色和库，确认数据库登录可用。先启动依赖，再初始化 Gateway：

```sh
wakeit gateway nats-init --dir /persistent/wakeit/nats
wakeit gateway migrate --config /persistent/wakeit/gateway.yaml --env-file /persistent/wakeit/gateway.env
wakeit gateway bootstrap --config /persistent/wakeit/gateway.yaml \
  --env-file /persistent/wakeit/gateway.env --username admin
```

bootstrap 密码取本机 WAKEIT_PASSWORD（至少 12 字节），用后清除。migrate 和 bootstrap 不放进自启服务，已有账号与数据库不重复重建。nats-init 只在新身份目录初始化一次；保留 NATS identity 与持久化目录。

服务生成不安装、不启动，生成后根据系统指南执行安装脚本：

```sh
wakeit gateway service --config /persistent/wakeit/gateway.yaml \
  --env-file /persistent/wakeit/gateway.env --user SERVICE_USER \
  --nats-bin /absolute/path/nats-server --nats-config /persistent/wakeit/nats/nats.conf \
  --output /persistent/wakeit/services
```

依赖外部 NATS 时省略两项 nats 参数，并验证外部服务常驻。postgres-service 指已有系统服务名；外部 PostgreSQL 不指定本机服务名。生成目录必须为空，防止覆盖现有服务配置。

服务运行账号必须能读配置/环境/NATS 凭据并写 NATS 持久化目录；系统启动不继承用户终端 PATH。使用绝对路径，把数据放在系统目录，避开 macOS 的受保护 Documents/Desktop。

## 验证与交付

检查系统服务实际加载、enabled/Automatic 状态及失败重启策略，再请求 http://127.0.0.1:8080/health/ready。返回 ready 才算依赖与 Gateway 都可用。注册测试 Runner 并在批准目录内执行无副作用任务，确认结果。

不为测试而擅自重启用户机器；能实际重启的环境验证重启后就绪，其他环境只报告已验证的服务配置/启动，并注明重启未实测。给出系统服务名、数据/日志路径、停止与卸载方法。卸载只移除服务，不删数据库、身份或 Agent 凭据。

默认日志保留 7 天、运行记录 90 天；升级先备份 PostgreSQL、NATS 数据和身份，再独立迁移与重启。安装使用官方 v0.2.0 Release 或源码构建，校验发布清单；不要使用 Actions 临时产物代替公开安装包。

## 多工作区与 GitHub 登录

当前 Gateway 支持独立工作区；详见仓库 `docs/workspaces.md`。自托管默认密码登录、关闭公开注册，有效邀请仍可注册。官方站配置 `auth.mode: github`，通过环境变量提供 OAuth App Client ID、Secret，回调为公开地址的 `/api/v1/auth/github/callback`；不把 Secret 写进配置仓库或对话。演示区只能使用合成数据，GitHub 登录后只读。不要把工作区 Admin 当作实例管理员；旧库升级保留 `0001`，应用新增迁移，再由运维显式指定实例管理员。
