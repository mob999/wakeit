# Wakeit skill

安装或更新完整 Wakeit Skill：

```sh
wakeit skill install
```

默认目录为 `~/.agents/skills/wakeit`，可用 `wakeit skill install --dir /path/to/agent/skills/wakeit` 覆盖。命令下载最新正式 CLI Release 中的 Skill 并校验，更新时替换目标目录内的文件。重载 Agent 后使用 `$wakeit`，或直接提出 Wakeit 部署、任务和适配需求。

入口会区分用户要连接已有 Gateway 还是自行部署。只加载当前需要的连接、部署、Runner、任务、运行、适配器或更新指南；一次普通任务创建不会加载整套运维文档。

例如：

- “连接这个 Gateway，查看最近失败的任务。”
- “在这台 Linux 服务器部署 Gateway，并保证开机自动启动。”
- “每天九点用这台 Runner 上的 Codex 检查仓库。”
- “我的 Agent 是异步 HTTP 服务，帮我写一个适配器。”

Skill 自包含，安装后无需保留 Wakeit 源码仓库；需要源码构建时按连接指南获取仓库。CLI 安装包及 skill 由 `cli-v*` Release 一同交付，也可直接使用仓库中的 skill 目录。

README 的维护技能另存于 `.agents/skills/readme-crafter-skill/`，用于仓库文档写作，不是产品的 Wakeit skill。

测试审查与重构使用 [.agents/skills/unit-test-review/SKILL.md](../.agents/skills/unit-test-review/SKILL.md)。该开发技能按行为与边界识别冗余，保留必要的集成保护；覆盖归属与命令见 [测试策略](test-strategy.md)。复制整个目录到 Agent 的 skills 目录后可使用 `$unit-test-review`。
