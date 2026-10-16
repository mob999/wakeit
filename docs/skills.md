# Wakeit skill

产品技能位于 `skills/wakeit/`。复制整个目录到所用 Agent 配置的 skills 目录；不要只复制 SKILL.md，入口按需读取同目录的 references 和 assets。

Codex 可通过 `$skill-installer` 指定 `mob999/wakeit` 的 `skills/wakeit` 路径安装。其他兼容 Agent Skills 的 Agent 按自身技能安装方式安装目录。重载技能后使用 `$wakeit`，或直接提出 Wakeit 部署、任务和适配需求。

入口会区分用户要连接已有 Gateway 还是自行部署。只加载当前需要的连接、部署、Runner、任务、运行、适配器或更新指南；一次普通任务创建不会加载整套运维文档。

例如：

- “连接这个 Gateway，查看最近失败的任务。”
- “在这台 Linux 服务器部署 Gateway，并保证开机自动启动。”
- “每天九点用这台 Runner 上的 Codex 检查仓库。”
- “我的 Agent 是异步 HTTP 服务，帮我写一个适配器。”

Skill 自包含，安装后无需保留 Wakeit 源码仓库；需要源码构建时按连接指南获取仓库。安装包及 skill 由正式 Release 工作流一同交付，也可直接使用仓库中的 skill 目录。

README 的维护技能另存于 `.agents/skills/readme-crafter-skill/`，用于仓库文档写作，不是产品的 Wakeit skill。
