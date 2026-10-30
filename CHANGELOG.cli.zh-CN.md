## 0.2.0

- 使用 `wakeit plugin` 从 Git 仓库或本地目录安装、更新第三方 Agent 插件。
- 官方 Agent 适配器预装提供，与第三方适配器采用相同的插件格式。
- CLI 安装和更新同时包含官方插件包，已有任务继续使用原来的 Agent ID。
- **破坏性变更：**移除 `wakeit adapter` 命令。官方 Agent YAML 请通过 `wakeit agent init` 重新生成；新注册的 Runner 直接引用插件资源。Gateway 无需升级。

## 0.1.1

- 使用 `wakeit skill install` 安装或更新 Wakeit Skill，支持默认位置和自定义目录。
- 精简安装和使用文档。

## 0.1.0

- 通过浏览器授权连接 Wakeit 工作区，并管理已保存的连接。
- 注册设备，使用内置 Runner 执行 Agent 任务，支持用户级自启并沿用已有 Agent 登录。
- 在命令行管理任务、预设、运行、设备和成员。
- 实时观察执行、读取结果，并继续受支持的 Agent 会话。
- 独立于 Gateway 安装和更新 CLI；Wakeit Skill 随 CLI 一同发布。

CLI 不负责部署 Gateway。自行托管服务请使用独立的 [Gateway 安装入口](https://github.com/mob999/wakeit/blob/cli-v0.1.0/docs/gateway-service.md)。
