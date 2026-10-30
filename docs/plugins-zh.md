# Agent 插件

[English](plugins.md)

CLI 0.2 预装包含全部官方 Agent 适配器的插件包。第三方插件安装在实际执行任务的 Runner 设备上：

```sh
wakeit plugin install https://github.com/example/wakeit-agent
wakeit plugin install git:github.com/example/wakeit-agent@v1.0.0
wakeit plugin install ./my-agent
wakeit plugin list
wakeit plugin update
wakeit plugin remove https://github.com/example/wakeit-agent
```

安装、更新、移除在 Runner 下次启动时生效，不会自动重启服务或中断任务。`list` 显示已安装的包和本机 Runner 的加载状态。自动化调用可添加 `--json`。

## 来源与版本

Git 包按仓库地址识别，忽略 ref 和 HTTPS／SSH 等写法差异。tag 和 commit 固定版本；未指定 ref 时跟随默认分支，指定分支时跟随该分支，仅在执行 `update` 时更新。重新安装同一来源的另一个 ref 即可切换版本。

本地包按绝对目录识别，直接引用目录。移除插件不删除作者文件，原目录已移动或删除时仍可取消安装记录。

包的 `name` 用于展示。任务仍选择 `kimi-code` 等 Agent ID。不同来源提供相同 Agent 时，显式选择要使用的来源：

```sh
wakeit plugin install https://github.com/example/custom-kimi --replace-agent kimi-code
```

选择对本机生效。普通独立 Agent YAML 仍可使用；重复 ID 会报告冲突，不会按加载顺序覆盖。

官方包随 CLI 安装和更新，用户显式安装的版本或移除选择不会被 CLI 更新覆盖。旧版本文件保留，供运行中的进程和已保存会话使用。Gateway 不安装或执行插件。

## 编写插件

在仓库根目录放置 `wakeit-plugin.json`：

```json
{
  "manifest_version": 1,
  "name": "example-agent",
  "version": "1.0.0",
  "wakeit": {
    "protocol": 1,
    "agents": ["agents/example.yaml"],
    "runtimes": ["node"]
  }
}
```

Agent YAML 沿用现有[适配协议](protocol.md)。`command: ./bin/example` 相对于包根目录解析；参数中的包内文件使用 `{{plugin.root}}/adapter.cjs`。进程工作目录仍是任务目录，`node` 等普通命令使用 PATH。脚本通过 `runtimes` 声明所需解释器，Wakeit 不运行构建钩子或自动安装依赖。

原生插件可在 `wakeit.artifacts` 中按平台提供 gzip tar 压缩包：

```json
{
  "aarch64-apple-darwin": {
    "url": "https://example.com/releases/v1.0.0/macos-arm64.tar.gz",
    "sha256": "<64 位十六进制校验值>"
  }
}
```

压缩包路径相对于包根目录，仅包含普通文件和目录。也可用 `checksums_url` 指向包含该产物文件名的 SHA256SUMS。平台无产物或缺少解释器时，安装会给出明确原因。完整清单格式见 `wakeit schema plugin`。

可选的 `wakeit.commands` 将命令名称映射到包内可执行路径，用 `wakeit plugin exec SOURCE COMMAND -- ARGS` 调用，保持当前工作目录。官方包提供 `adapter` 入口，用于直接检测或查询会话，例如：

```sh
wakeit plugin exec git:github.com/mob999/wakeit adapter -- detect kimi-code
```

## 从 CLI 0.1 升级

通过[安装脚本](install-zh.md)安装完整的 CLI 0.2 包。旧 `wakeit adapter` 命令已移除。若 Runner 显式列出了旧官方 YAML，用 `wakeit agent init ID --file NEW_FILE` 重新生成并更新其 `agents` 路径，或者设置 `bundled_agents: true` 并将 `agents` 设为空列表。需要的自定义 Agent 选项请保留。

新 setup／enroll 直接使用插件资源。保留原设备凭据和 Runner 身份，Gateway 无需升级。新的插件会话保留其使用版本的可执行文件路径。
