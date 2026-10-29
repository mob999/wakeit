# 内置 Agent 真实验收

2026-10-16 对全部 12 个内置模板逐项调用。事件协议的合成测试通过，不等于真实 CLI、认证和工具调用已经通过；下表分开记录。所有测试使用独立验收目录，不执行用户任务，不更改原有 Agent 登录或模型配置。

| Agent / 原生版本 | 公开消息与工具过程 | 最终报告 | 命名、列表、绑定及续聊 | 模型目录 | 实际认证 |
|---|---|---|---|---|---|
| Codex 0.162.0-alpha.2 | 实时通过 | 通过，单次上报 | 全部通过 | 通过 | 本机已有配置 |
| Claude Code 2.1.301 | 实时通过 | 通过，单次上报 | 全部通过 | 未声明 | Magpie |
| Pi 1.2.0 | 实时通过 | 通过 | 全部通过 | 通过 | 本机 Magpie |
| OpenCode 2.0.14 | 工具完成后才收到节点 | 通过 | 全部通过 | 通过 | 本机已有配置 |
| Kimi Code 2.1.1 | 实时通过 | 通过 | 全部通过 | 未声明 | 本机已有配置 |
| Goose 1.54.0 | 实时通过 | 通过 | 未接入 | 未声明 | Magpie |
| Cline 3.0.72 | 实时通过 | 通过 | 未接入 | 未声明 | Magpie，`openai` provider |
| Gemini CLI 0.64.0 | 公开消息及委派节点实时通过，子命令输出未核实 | 通过 | 未接入 | 未声明 | Magpie |
| Qwen Code 0.25.2 | 实时通过 | 通过 | 未接入 | 未声明 | Magpie，Kimi 模型 |
| Copilot CLI 1.0.98 | 实时通过，另验证拒绝工具 | 通过 | 未接入 | 未声明 | Magpie，BYOK Responses |
| Cursor 2026.10.14-941492e | 认证阻断，未通过真实调用 | 未验证 | 未接入 | 未声明 | Magpie 不被当作平台凭据接受 |
| Antigravity CLI 1.4.0 | 检测通过，推理需原生登录 | 未验证 | 未接入 | 接口声明，认证阻断 | 未找到可验证的 Magpie 接入方式 |

“未接入”表示 Wakeit 模板未声明完整会话能力，不能命名、选择绑定或生成受校验的续聊入口；不代表原生 Agent 没有这些功能。OpenCode 当前 JSON 命令只在工具完成后输出节点，不能把事后的起止时间称作实时开始事件。Cursor、Antigravity 的合成协议测试不替代有账号后的真实验收。

## 验收方法

1. 安装或定位各原生 CLI，记录版本并调用模板 `detect`。新增测试 CLI 的安装、HOME、配置、PATH 与日常 Runner 隔离。
2. 按 YAML 的命令及独立参数调用 `submit`，传入中文任务名和唯一上下文标记。要求 Agent 先回复 STARTING，执行 `printf AUDIT_TOOL; sleep 2; printf AUDIT_DONE`，然后给出 AUDIT_OK。禁止读取或改动文件。
3. 在子进程仍运行时逐行读取统一 JSONL，核对工具实际输出包含 AUDIT_DONE、运行节点早于完成节点、最终报告只出现一次。不能用模型口头声称完成替代真实工具结果，也不能把工具输出拼成报告。
4. 对声明会话能力的模板，使用原生接口保存 `任务名-wakeit-时间戳`，查询列表、精确检查完整 ID、持久化状态及目录，然后用同一 ID 续聊并要求回忆唯一标记。再次检查 ID 和原标题不变；模型目录仅对声明接口的模板调用。
5. 对未登录 Agent，仅向其明确支持的自定义 Provider 注入测试代理配置。测试结束清除复制的认证和临时服务；不将代理密钥冒充平台令牌发送给其他服务。保留的验收摘要只包含布尔结果、版本及计数，不含模型密钥、真实用户会话或正文。

## Magpie 兼容性与本轮修正

主要使用 `codex/gpt-6.1-sol`。Claude 使用显式自定义 Anthropic 地址，Gemini 使用显式 API 地址；Goose 使用 OpenAI 兼容配置。Gemini 测试仅信任专用验收目录，没有放宽生产模板。它通过内部 generalist Agent 委派工具，主流的完成事件未携带输出；再次要求直接调用 shell 时原生 Agent 表示无该工具，因此仅确认公开消息、委派节点和最终报告传输，不能声称已验证底层 shell 命令成功。Qwen 的该模型首轮没有真正执行命令，改用 `kimi-code-cn/kimi-for-coding` 后验证真实工具输出与实时节点通过，不能把第一次仅返回 AUDIT_OK 记为通过。

Cline 的 `openai-native` provider 在代理上返回不支持的参数；`openai` 兼容 provider 通过。原生 3.0.72 管道输入无法可靠开始任务，因此模板改用 `--` 后的独立指令参数，仍不经过 shell。

Copilot 使用原生 CLI `--output-format json`，不使用 `--silent`，也不套用 SDK/workflow 格式。默认无人值守策略拒绝工具时，过程正确显示失败；正向测试在临时调用中明确预授权 shell 后通过两秒命令。生产模板没有添加任何允许所有工具的参数，实际权限继续由用户的原生配置决定。完整事件契约见 [Copilot fixture](../crates/wakeit-cli/tests/fixtures/copilot/README.md)。

Kimi 从聚合的 print 输出改用自有 loopback Server API 子进程，确认 WebSocket 订阅和心跳后执行；新建时原生保存标题，冷会话绑定先加载原生上下文。真实试跑确认工具在完成前出现、上下文续聊和标题持久化。Codex 与 Claude 的最终结果统一延后到原生完成及成功退出后输出一次。Antigravity 帮助输出在 stderr，检测现在兼容该行为并保留退出码校验。

## 自动回归与限制

`cargo test --all-targets` 覆盖独立 argv/stdin、中文分片、实时转发、重复及乱序节点、隐藏推理过滤、失败退出、缺少终态、进程取消、会话互斥和持久化。`cargo clippy --all-targets -- -D warnings` 检查 Rust 代码。Copilot 合成 JSONL 及 Kimi 本地服务 fixture 不包含真实录制内容；界面采用本地人工验收，不维护浏览器测试套件。

本轮没有对每个真实 Agent 注入网络故障、取消或离线恢复；这些路径的保证来自现有自动回归，不能声称每个平台、每个原生版本的真实故障组合均已验收。新增 CLI 版本应重新执行上述测试，尤其是 Cursor 与 Antigravity 补齐原生登录后。
