# Gateway 系统服务

Gateway 0.4.0 使用独立的 `wakeit-gateway`，不依赖 CLI 安装。CLI/Runner 0.1.0 的 `wakeit` 只负责接入和操作 Wakeit。

## 安装程序

macOS / Linux GNU：

```sh
curl -fsSL https://raw.githubusercontent.com/mob999/wakeit/main/deploy-gateway.sh | sh -s -- --version 0.4.0
```

脚本显示下载、校验和安装进度，默认安装到 `~/.local/bin`，可通过 `--bin-dir` 指定目录。已有 PostgreSQL/NATS 和配置时，可传 `--config`、`--env-file` 应用数据库更新，并用 `--start` 前台启动；不传配置时只安装程序。全栈部署继续使用仓库的 Docker Compose。

源码构建需要 Rust 1.89+ 和 Node.js 22.12+：

```sh
npm --prefix web ci
npm --prefix web run build
cargo build --locked --release -p wakeit-gateway
```

Windows 下载 `gateway-v*` Release 的 `wakeit-gateway-x86_64-pc-windows-msvc.exe`，按 `wakeit-gateway-release.json` 与 `SHA256SUMS` 核对后安装为 `wakeit-gateway.exe`。

## 系统自启

Gateway 自启覆盖操作系统启动后、用户未登录的场景。生成配置不会安装或启动服务；安装脚本需要对应系统管理员权限。Runner 另按 Agent 已登录的身份运行。

```sh
wakeit-gateway service --config /absolute/gateway.yaml \
  --env-file /absolute/gateway.env --user SERVICE_USER \
  --nats-bin /absolute/nats-server --nats-config /absolute/nats/nats.conf \
  --output /absolute/services
```

`--platform linux|macos|windows` 默认当前系统；环境文件必须存在、定义配置中指定的数据库 URL 变量，支持 dotenv 引号，不执行 shell。它在 Tokio 启动前加载，显式文件值覆盖进程继承值；生成配置只记录文件路径，不嵌入秘密。

`--nats-bin` 和 `--nats-config` 成对提供，生成附加 NATS 系统服务；外部 NATS 可省略。`--postgres-service` 在 Linux/Windows 指定已有 PostgreSQL 系统服务依赖；macOS 的 PostgreSQL 由独立系统 LaunchDaemon 启动，Gateway 失败重启等待依赖就绪。数据库安装、角色和库初始化与系统服务安装分开，migrate/bootstrap 不放入开机启动流程。输出目录必须为空。

| 平台 | 开机机制 | 输出与管理 |
|---|---|---|
| Linux | 系统 systemd，multi-user.target | `.service`、install/uninstall.sh；journalctl 查看日志 |
| macOS | `/Library/LaunchDaemons`，system 域 | `.plist`、install/uninstall.sh；`/Library/Logs/Wakeit` 日志 |
| Windows x64 | WinSW Automatic 系统服务 | `.xml`、install/uninstall.ps1；服务目录中的日志 |

安装前检查服务账号的读写权限、二进制绝对路径和已有同名服务。Linux 启用 PostgreSQL 系统服务；macOS 需要系统级数据库 LaunchDaemon；Windows 将 PostgreSQL 服务设为 Automatic。外部依赖复核其常驻/高可用配置。

平台操作步骤随产品 skill 一起维护：

- [Linux 部署](../skills/wakeit/references/deploy-linux.md)
- [macOS 部署](../skills/wakeit/references/deploy-macos.md)
- [Windows 部署](../skills/wakeit/references/deploy-windows.md)

受限 gateway.env 示例：

```dotenv
WAKEIT_DATABASE_URL='postgres://wakeit:URI_SAFE_PASSWORD@127.0.0.1:5432/wakeit'
```

原生命令也可指定文件：

```sh
wakeit-gateway migrate --config gateway.yaml --env-file gateway.env
wakeit-gateway bootstrap --config gateway.yaml --env-file gateway.env --username admin
wakeit-gateway start --config gateway.yaml --env-file gateway.env
```

CLI 解析失败不会输出环境文件内容；管理员密码仍由一次性 WAKEIT_PASSWORD 环境变量提供。数据库 URL 和密码不进入生成的 service/plist/XML。

检查服务加载及自启状态后访问 `/health/ready`，验证依赖和 Gateway 真正就绪。系统配置检查不能代替真实重启验收。卸载只移除系统服务，不删除账号、任务、数据库、NATS 身份或 Agent 凭据。

Docker Compose 是另一种部署方式：Linux 启用 Docker 系统服务且整个栈具备 restart 策略；macOS/Windows 的 Docker Desktop 用户登录启动不宣称无人登录服务已完成。
