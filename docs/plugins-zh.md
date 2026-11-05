# Agent 插件

[English](plugins.md)

CLI 预装官方 Agent 插件包。以下 SDK 接入方式描述当前源码中的契约，不表示已发布的 CLI 版本均已支持；面向已安装版本时，以该版本的 schema 和能力为准。

安装方式不变，第三方插件仍安装在实际执行任务的 Runner 设备上：

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
    "runtimes": ["node"],
    "plugins": {
      "example": {
        "name": "Example plugin",
        "command": "node",
        "args": ["{{plugin.root}}/plugin.js"],
        "agents": {
          "example": {
            "name": "Example Agent",
            "mode": "sync",
            "parameters": {"model": {"type": "string", "label": "Model"}},
            "operations": ["detect", "execute"]
          }
        }
      }
    }
  }
}
```

`wakeit.plugins` 为每个插件声明一个入口，可同时包含 Agent、事件处理器、Hook、动作和 Provider。`agents` 映射的键定义 Agent ID，Descriptor 声明名称、可选 logo、参数和真实支持的操作。程序通过 SDK 的 `Plugin.agent(id, Agent)` 注册回调；初始化时校验注册与声明一致。Agent 不是普通 action，不能以动作代替 Agent 执行。实现方式见[插件 SDK](plugin-sdk-zh.md)。

`command: ./bin/example` 相对于包根目录解析；参数中的包内文件使用 `{{plugin.root}}/plugin.js`。任务工作目录通过 Agent 请求传入，`node` 等普通命令使用 PATH。脚本通过 `runtimes` 声明所需解释器，Wakeit 不运行构建钩子或自动安装依赖。

### 旧格式兼容

用户已有的独立 Agent YAML 和 `wakeit.agents: {"example": "agents/example.yaml"}` 继续沿用[命令适配协议](protocol.md)。清单定义 Agent ID，包内旧 YAML 可省略 `id`。旧 `wakeit.extensions` 作为插件声明的兼容输入；新包使用 `wakeit.plugins`。不要在新旧字段中重复声明同一插件或 Agent。

官方包使用 `wakeit.plugins.official` 的静态 Descriptor，并以 `args: ["sdk"]` 启动统一入口，不需要生成或读取官方 Agent YAML。旧 CLI 可能拒绝新清单；不要依赖静默忽略字段或降级为普通动作。

原生插件可在 `wakeit.artifacts` 中按平台提供，或通过 `wakeit.artifact` 使用统一 URL 模板声明 gzip tar 压缩包：

```json
{
  "aarch64-apple-darwin": {
    "url": "https://example.com/releases/v1.0.0/macos-arm64.tar.gz",
    "sha256": "<64 位十六进制校验值>"
  }
}
```

压缩包路径相对于包根目录，仅包含普通文件和目录。平台压缩包补充可执行资源，不能覆盖所选 Git 版本的清单或兼容格式的 Agent YAML。也可用 `checksums_url` 指向包含该产物文件名的 SHA256SUMS。平台无产物或缺少解释器时，安装会给出明确原因。URL 可使用清单版本 `{version}` 和当前平台 `{target}`。用 `wakeit plugin validate --dir ./my-agent` 校验完整性；完整清单格式见 `wakeit schema plugin`。

可选的 `wakeit.commands` 将命令名称映射到包内可执行路径，用 `wakeit plugin exec SOURCE COMMAND -- ARGS` 调用，保持当前工作目录。官方包提供 `adapter` 入口，用于直接检测或查询会话，例如：

```sh
wakeit plugin exec git:github.com/mob999/wakeit adapter -- detect kimi-code
```

## 从 CLI 0.1 升级

通过[安装脚本](install-zh.md)安装完整 CLI 包。旧 `wakeit adapter` 命令已移除。使用官方 SDK Agent 时设置 `bundled_agents: true`，并从 Runner 的 `agents` 路径列表移除对应的旧官方定义，避免同一 ID 重复加载。独立自定义 YAML 仍可保留；若只使用官方包，`agents` 可为空列表。

SDK Agent 不需要 `wakeit agent init` 生成 YAML，该命令仅适用于仍以 YAML 声明的兼容适配器。保留实际需要的自定义配置，不覆盖设备身份或凭据。

新 setup／enroll 直接使用插件资源。保留原设备凭据和 Runner 身份，Gateway 无需升级。新的插件会话保留其使用版本的可执行文件路径。

官方适配器新建与恢复均走 SDK Agent 回调，由适配器构造原生命令的独立 argv；普通 `adapter -- run …` 入口仅供独立协议诊断，不是官方注册路径。Kimi 通过原生 ACP stdio 执行。已有原生会话标题保留；已验证会话无标题时，Runner 保存本地显示名并用于检查和搜索，之后出现原生标题时优先显示。第三方命令和事件接口不变。

## 通用扩展

插件包也可以提供事件监听、生命周期 Hook 和设备配置的动作。参见[插件 SDK](plugin-sdk-zh.md)。
