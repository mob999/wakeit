# Wakeit

[English](README.md) | 简体中文

让 Agent 在自己的设备上按时完成任务，集中查看执行结果。

Wakeit 将你已有的 Codex、Claude Code、Kimi Code、Pi 等 Agent 接入定时任务。**Gateway** 提供 WebUI、任务调度和执行历史；每台设备上的 **Runner** 调用本机 Agent 执行任务。设备主动连接服务，无需开放入站端口。

## 能做什么

- 手动执行，或按一次性时间、固定间隔、Cron 定时运行任务。
- 将多个 Agent 编排成 DAG，在依赖步骤之间传递结果。
- 从可复用的预设创建任务，填写所需参数。
- 查看执行进度、阅读报告，并继续受支持的 Agent 会话。
- 在不同工作区管理设备、任务和访问权限。

Runner 支持 Linux、macOS 和 Windows。参见[支持的 Agent 与能力](docs/agents.md)。

## 快速开始

### 1. 准备 Gateway

使用已有 Gateway，或按[部署指南](docs/operations.md)通过 Docker Compose 自行托管。原生安装方式见 [Gateway 系统服务](docs/gateway-service.md)。

### 2. 接入设备

在设备上安装并登录你要使用的 Agent，然后安装 Wakeit CLI。macOS 或 Linux GNU：

```sh
curl -fsSL https://raw.githubusercontent.com/mob999/wakeit/main/install.sh | sh
```

打开新终端，连接 Gateway。将 `WORKSPACE_ID` 替换为 WebUI 地址中的工作区 UUID，并指定允许执行任务的目录：

```sh
wakeit setup --gateway https://wakeit.example.com \
  --workspace WORKSPACE_ID --allow-root /path/to/projects
```

在浏览器中授权后，setup 会注册 Runner 并配置用户级自启。[Windows 及其他安装方式 →](docs/install-zh.md)

也可以打开 WebUI 中的 **Agent Prompt**，交给 Agent 完成配置。[Wakeit skill](docs/skills.md) 可以让 Agent 帮你管理任务和运行。

### 3. 创建任务

在 WebUI 点击 **+**，选择任务、DAG 或预设。选择设备、Agent 和工作目录，填写指令并设置调度。保存或立即执行，在运行页查看进度和结果。

使用 CLI 时，可从[任务示例](examples/task.yaml)和 [CLI 指南](docs/cli.md)开始。

## 更新

```sh
wakeit update
```

更新后重启 Runner。Gateway/WebUI 与 CLI/Runner 独立发布；Gateway 按所用的[部署方式](docs/operations.md)更新。

## 文档

| 主题 | 文档 |
|---|---|
| 安装与部署 | [CLI](docs/install-zh.md) · [Gateway](docs/operations.md) |
| 任务、运行与自动化 | [CLI](docs/cli.md) · [DAG](docs/dag.md) · [预设](docs/templates.md) |
| Agent 与自定义适配 | [支持的 Agent](docs/agents.md) · [适配协议](docs/protocol.md) |
| 团队与权限 | [工作区](docs/workspaces.md) |
| 开发与集成 | [开发指南](CONTRIBUTING.md) · [REST API](docs/api.md) |
| 版本记录 | [Gateway](CHANGELOG.zh-CN.md) · [CLI](CHANGELOG.cli.zh-CN.md) |

[Apache-2.0](LICENSE) · [第三方组件](docs/third-party.md)
