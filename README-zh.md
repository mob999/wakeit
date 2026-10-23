# Wakeit

[English](README.md) | 简体中文

让 Agent 在指定时间、指定设备上完成任务。

你定义指令和时间，Wakeit 将任务交给设备上的 Runner。Gateway 集中管理调度、工作区成员和执行历史；WebUI 展示过程、日志和结果。

- **接入已有 Agent**：内置 Codex、Claude Code、Pi、OpenCode、Kimi Code、Antigravity CLI、Gemini CLI、Cursor、GitHub Copilot CLI、Cline、Goose、Qwen Code；[能力与配置](docs/agents.md)按原生接口提供，自定义适配只需配置命令。
- **在自己的设备执行**：Runner 主动连接 Gateway，支持 Linux、macOS、Windows，无需设备开放入站端口。
- **按时运行，查看结果**：Cron、固定间隔、一次性时间和手动执行；支持实时过程及 Markdown 报告，包含代码高亮、公式与 Mermaid 图表；动态以紧凑时间线汇总结果，提供原文目录、报告中的文件链接和 DAG 输出切换；可通过原 Runner 预览报告链接中的受支持本地文件，访问受设备目录范围限制。
- **编排 Agent DAG。** 在画布拖入 Agent，或导入一份 YAML，连接 AND 依赖并引用上游结果；整图执行并按所选节点展示报告，节点可配置指数退避重试。见 [DAG 任务](docs/dag.md)。
- **保留执行上下文**：支持会话的 Agent 可绑定原生会话，直接通过网页终端续聊、复制原生命令，或在原 Runner 设备用 `wakeit run resume <run-id>` 继续交互；见[会话续聊](docs/cli.md#绑定会话与本机续聊)。
- **隔离不同工作区**：按团队隔离任务、运行、设备和权限，见[工作区配置与认证](docs/workspaces.md)。
- **本地或云端托管**：一个 Rust 二进制包含 Gateway、Runner 和 CLI，WebUI 随 Gateway 交付。

最新已发布版本为 **0.3.3**，使用 Apache-2.0 许可。请使用与 Gateway 匹配的 CLI 版本。接口与数据结构仍可能调整。

报告和 Markdown 文件预览支持以缩略图内嵌本地 PNG/JPEG/WebP 图片，由原 Runner 读取；相对路径按执行目录或 Markdown 所在目录解析，沿用工作区权限及目录范围。点击图片后按需加载原图。

## 安装

从 [v0.3.3 Release](https://github.com/mob999/wakeit/releases/tag/v0.3.3) 下载当前系统和架构的二进制，同时下载 `wakeit-release.json` 与 `SHA256SUMS`，核对 SHA-256 后安装到用户 bin 目录。支持 Linux、macOS 和 Windows。

从源码构建需要 Rust 1.89+、Node.js 22.12+：

```sh
git clone https://github.com/mob999/wakeit.git
cd wakeit
npm --prefix web ci
npm --prefix web run build
cargo install --locked --path crates/wakeit
```

## 接入已有 Gateway

已有 Gateway 时，点击 WebUI 顶栏的 `{}` 复制 Agent Prompt，交给 Agent 完成 CLI 与 Wakeit skill 安装、浏览器授权请求、设备注册和用户级自启；你在浏览器中核对并授权。

将 `WORKSPACE_ID` 替换为工作区 WebUI 地址中的 UUID：

```sh
wakeit setup --gateway https://wakeit.example.com --workspace WORKSPACE_ID
# 可选：--runner-name dev-server --allow-root /path/to/projects
wakeit connection list
wakeit runner list
```

默认设备名是主机名，执行范围是当前工作目录。管理员和读写成员可注册 Runner，只有管理员可吊销。已安装并登录的 Agent 保持原样，setup 不会替你创建或运行任务。

只需 CLI 时使用 `wakeit login --gateway URL --workspace WORKSPACE_ID`，只读成员同样可用。连接按 Gateway、工作区和账号保存在平台标准配置目录，跨目录生效；API Key 默认不过期，可在设置 → API Key 撤销。`wakeit connection use NAME` 切换已有连接。显式参数和 `WAKEIT_*` 环境变量仍可使用。

工作区管理员在设置 → 成员中管理成员关系和邀请。实例管理员通过实例账号管理页面创建、启停全局账号及重置本地密码。停用账号或重置密码会撤销该账号的登录会话和 API Key。

## 自行部署 Gateway

单机部署需要 Docker Compose v2。在仓库根目录执行：

```sh
cp .env.example .env
# 将 .env 中的 WAKEIT_POSTGRES_PASSWORD 改为仅含字母和数字的随机密码。
docker compose build
docker compose up -d
export WAKEIT_PASSWORD='replace-with-a-generated-admin-password'
docker compose run --rm -e WAKEIT_PASSWORD gateway \
  gateway bootstrap --config /app/gateway.yaml --username admin
unset WAKEIT_PASSWORD
```

打开 <http://localhost:8080> 登录。Compose 会持久化 PostgreSQL、NATS 数据和设备身份，并自动重启服务；Linux 还需启用 Docker 的系统自启。

macOS、Windows 的 Docker Desktop 登录启动不能代替无人登录时的系统启动。需要系统级开机运行时，使用[原生 Gateway 服务指南](docs/gateway-service.md)。跨设备部署同时配置 Gateway 的公开地址及 NATS WSS 地址，见[部署与运维](docs/operations.md)。

## 创建首个任务

在设备上安装并登录需要使用的 Agent，再运行 `wakeit setup --gateway URL --workspace WORKSPACE_ID`。用户级自启安装后可关闭当前终端。执行范围由 `--allow-root` 指定，未配置范围时不能浏览目录或执行任务。手动注册及自启方式见 [Runner 指南](skills/wakeit/references/runner.md)。

在 WebUI 新建任务：填写名称和指令，选择 Runner、目录、Agent 与模型，再进入“时间”页设置调度并保存。

CLI 可导入[任务示例](examples/task.yaml)。修改示例中的设备、目录和指令后：

```sh
wakeit task apply --file examples/task.yaml
wakeit --json task trigger daily-review
wakeit run watch <run-id>
wakeit task pause daily-review
```

Task 的权威存储是 Gateway 的 PostgreSQL；YAML 是提交或更新任务的输入文件。模型发现取决于适配提供的接口，没有接口时手填。普通任务失败不会自动重新执行；DAG 节点可通过 `max_retries` 启用有限次数的重试，并可立即启动已排队的重试，不增加尝试次数；无法确认执行结果时进入 `unknown`，先核对实际状态，再决定是否重试。

使用 `wakeit run retry RUN --node NODE` 可在原 Run 内重试失败节点，保留成功节点并继续下游。DAG 整图总时限默认关闭，可按需开启。

任务可在详情中归档，停止后续调度，正在执行的运行继续，历史保留；设置 → 归档任务可恢复，恢复后保持暂停。CLI 对应命令为 `wakeit task archive <id>`、`wakeit task list --archived` 和 `wakeit task restore <id>`。

## 让 Agent 帮你使用 Wakeit

仓库提供可独立安装的 [Wakeit skill](skills/wakeit/SKILL.md)。将整个 `skills/wakeit` 目录安装到所用 Agent 的技能目录，再调用 `$wakeit`：

> 使用已有 Gateway，注册这台设备，并创建每天早上九点检查仓库的任务。

Skill 按需加载连接、部署、Runner、任务、运行、适配器和更新指南，区分连接已有服务与部署新 Gateway。安装方式见 [Skill 使用说明](docs/skills.md)。

## 更新

```sh
wakeit update --check
wakeit update
```

通过公开 GitHub Releases 检查正式版本，无需 GitHub 登录。普通更新排除预览版，并按正式版本号判断更新；同版本提交不会触发更新。校验通过后只替换二进制，保留数据和配置，也不自动重启运行中的服务。

## 文档

| 需要做什么 | 文档 |
|---|---|
| 使用 CLI 管理任务和运行 | [CLI 指南](docs/cli.md) |
| 配置工作区、成员与 GitHub 登录 | [工作区](docs/workspaces.md) |
| 配置开机自启 | [Gateway 系统服务](docs/gateway-service.md) |
| 部署、备份、恢复和升级 | [部署与运维](docs/operations.md) |
| 开发 Agent 适配 | [配置与适配协议](docs/protocol.md) |
| 调用接口 | [REST API](docs/api.md) |
| 构建和运行测试 | [开发指南](CONTRIBUTING.md) |
| 查看任务模板与动态 DAG 提案 | [任务模板 RFC](docs/rfc-2-task-templates.md) |

许可证：[Apache-2.0](LICENSE)。第三方技能及 Windows 服务工具的来源见[第三方组件](docs/third-party.md)。
