## 0.1.1

### 新增

- WebUI 支持中英文切换，并记住语言偏好。
- 内置 Kimi Code、Antigravity CLI、Gemini CLI、Cursor、GitHub Copilot CLI、Cline、Goose 和 Qwen Code 适配，按各原生接口提供对应能力。
- 点击版本号查看中英文更新日志。

### 改进

- 英文 Agent Prompt 引导 CLI 接入及 Wakeit skill 安装。
- Agent 详情使用统一表格和官方矢量图标；新增操作统一使用加号，运行重试与刷新图标区分，任务选择列表隐藏未检测到的 Agent。

### 修复

- Kimi 实时展示执行过程，并为新任务会话保存标题；续聊已有会话时保留原标题和上下文。
- Copilot 展示 JSONL 执行事件，Cline 可靠传递任务指令，Antigravity 检测兼容输出到 stderr 的原生帮助。
- Codex 和 Claude 在成功完成后仅上报一次最终结果。
- 查询公开版本无需 GitHub 登录。

## 0.1.0

- 支持 Cron、固定间隔、一次性和手动执行 Agent 任务。
- 在已注册的设备上执行任务，可配置工作目录、Agent 参数，并在适配支持时选择模型。
- 查看实时执行过程、日志和 Markdown 结果，通过动态时间线浏览已完成的运行。
- 为支持的 Agent 绑定已有会话并续聊。
- 在 WebUI 管理成员、API Key、设备和归档任务，提供管理员、操作员和查看者角色。
- 通过浏览器授权连接 CLI、注册 Runner，支持用户级自启和内置 Agent 适配。
