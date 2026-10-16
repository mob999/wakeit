# 内置 Agent 适配

Runner 调用设备上已安装、已登录的原生 CLI，不安装 Agent、不复制模型凭据。所有模板与第三方适配使用同一配置及事件协议。

| ID | 原生命令 | 实时过程 | 模型选择 | 会话绑定与续聊 |
|---|---|---|---|---|
| `codex` | `codex` | 支持 | 原生查询或手填 | 支持 |
| `claude-code` | `claude` | 支持 | 手填 | 支持 |
| `pi` | `pi` | 支持 | 原生查询或手填 | 支持 |
| `opencode` | `opencode` | 支持 | 原生查询或手填 | 支持 |
| `kimi-code` | `kimi`，Kimi Code 2.x | 支持 | 手填 | 只读查询原生会话、精确绑定及原生命令续聊 |
| `agy` | Google Antigravity CLI 的 `agy` | 支持 | 原生 `models` 查询或手填 | 本轮不声明 |
| `gemini-cli` | `gemini` | 支持 | 手填 | 本轮不声明 |
| `cursor` | `cursor-agent` | 支持 | 手填 | 不声明 |
| `copilot` | `copilot` | 文本输出 | 手填 | 不声明 |
| `cline` | `cline` | 支持 | 手填 | 不声明 |
| `goose` | `goose`（AAIF / Block） | 支持 | 手填 | 不声明 |
| `qwen-code` | `qwen` | 支持 | 手填 | 不声明 |

没有模型接口时只显示输入框；查询失败或空列表也不会用静态目录补齐。模型 ID、推理强度和原生权限由实际 CLI 决定，配置目录不是账号调用权限的保证。

## 导出和启用

新设备通过 `wakeit setup` 导出全部内置模板。已有 Runner 不自动覆盖或改写适配文件；可导出新模板，将路径加入本机 `runner.yaml` 的 `agents` 数组，再重启对应用户级 Runner 服务：

```sh
wakeit agent init kimi-code --file kimi-code.yaml
wakeit agent init agy --file agy.yaml
wakeit agent init gemini-cli --file gemini-cli.yaml
wakeit agent init cursor --file cursor.yaml
wakeit agent init copilot --file copilot.yaml
wakeit agent init cline --file cline.yaml
wakeit agent init goose --file goose.yaml
wakeit agent init qwen-code --file qwen-code.yaml
wakeit agent validate --file kimi-code.yaml
```

使用已安装并登录对应 Agent 的系统用户。需要测试实际推理时，在允许的测试目录运行 `wakeit agent test --file ...`；该命令会调用 Agent，可能产生费用或修改目录。只检查安装和命令能力可使用：

```sh
wakeit adapter detect kimi-code
wakeit adapter detect agy
wakeit adapter detect gemini-cli
```

这些探测只运行原生 `--help` / `--version`，不会启动模型会话。Kimi 要求当前 TypeScript Kimi Code 2.x，不把已归档的 Python `kimi-cli` 视为同一协议；Antigravity 探测也会拒绝缺少 headless 参数的同名桌面启动器。

## Kimi Code

依据 [MoonshotAI/kimi-code](https://github.com/MoonshotAI/kimi-code) 的当前命令行协议，使用 `kimi --prompt TEXT --output-format stream-json`；完整指令作为单独参数传递，不通过 shell 拼接。原生接口未提供本适配可用的标准输入指令入口，超长指令仍受操作系统命令行长度限制，尤其是 Windows。

消息和工具调用转换为公开过程；`session.resume_hint` 提供实际完整会话 ID。会话列表只读 `KIMI_CODE_HOME`（默认用户目录的 `.kimi-code`）下的 `session_index.jsonl`，仅对当前真实工作目录读取 `state.json`，确认 ID、目录及持久化状态。标题使用原生保存值，不从第一条 query 或日志推测，也不伪造 Wakeit 标题。原生不存在或目录不符时，绑定失败，不退回新建。

原生 provider 查询会初始化或刷新配置，因此本轮没有 `models` 接口；模型手填。适配不读取认证配置，不启动 SDK 做会话查询，也不修改原生会话元数据。详见[固定版本的协议来源](../crates/wakeit/tests/fixtures/kimi-code/README.md)。

## Antigravity CLI（agy）

依据 [Google 官方 headless 文档](https://antigravity.google/docs/cli/headless/)：Wakeit 将指令编码为单个 JSON stdin 事件，使用 `--input-format stream-json --output-format stream-json --disable-slash-commands`，保留中文、多行和引号，不把指令内容解释为 slash command。stdout 的 `init`、`step_update`、`result` 转换为会话元数据、公开过程与最终报告，工具输出不冒充报告。

模型查询使用 `agy --output-format json models`，只解析实际返回的目录；全局参数在子命令之前，见[上游确认](https://github.com/google-antigravity/antigravity-cli/issues/777)。模板不设置跳过权限的参数，沿用原生权限策略；需要确认而未获授权的工具可能被原生拒绝，用户应按 Agent 的实际配置处理。

虽然原生支持 `--conversation ID`，本轮未验证可满足精确 ID、真实目录和持久化检查的机器接口，因此不开放 Wakeit 的会话绑定或续聊入口，也不把 `init` 事件直接当作已持久化证明。未打包 Antigravity 二进制，其使用遵循 Google 原生条款。

## Gemini CLI

使用[官方 headless 流式输出](https://geminicli.com/docs/cli/headless/)，通过标准输入传递指令，并显式设置 `--prompt '' --output-format stream-json`。工具执行遵循已安装 CLI 的权限策略，不添加 `--yolo`。消息增量合并成完整快照，工具与命令分开显示；只有原生成功终态对应的最终消息作为报告。缺少终态、原生失败或输出中断不伪报成功。

本轮没有真实模型发现接口，也没有足够稳定的精确会话持久化检查，因此只提供手填模型及独立执行。Google 已宣布 [Gemini CLI 向 Antigravity CLI 过渡](https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli)；此模板面向仍在使用 `gemini` 的设备，不假设两者的事件格式兼容。

## Cursor、GitHub Copilot CLI 与 Qwen Code

[Cursor 官方输出协议](https://cursor.com/docs/cli/reference/output-format)提供完整消息、工具开始与完成、最终 `result`；适配使用 `cursor-agent --print --output-format stream-json`，按稳定工具 ID 更新过程。官方安装器仍提供 `cursor-agent` 别名，避免与编辑器的 `cursor` 命令混淆。不添加 `--force`：按照[原生 headless 约定](https://cursor.com/docs/cli/headless)，默认不强制应用文件修改；这不等同操作系统只读沙箱，需要写入的用户应明确配置原生权限。

[GitHub Copilot CLI](https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli/overview)使用原始标准输入传递指令，`--silent --no-ask-user --output-format text` 只返回最终文本。它不是旧版 `gh copilot` 扩展，不默认添加允许所有工具或扩大目录范围的参数。保留原生工具权限：无人值守时需要的权限应事先明确配置。没有公开验证的普通 prompt JSONL 事件契约，因此不把 SDK 事件套作 CLI Trace。

[Qwen Code headless 协议](https://github.com/QwenLM/qwen-code/blob/main/docs/users/features/headless.md)的完整 assistant 消息、tool_use / tool_result 和最终 result 使用独立适配解析，不沿用 Gemini 的事件格式。只有成功终态提供最终报告；工具输出、未完成消息和隐藏推理不冒充报告。

三者模型手填。原生 CLI 提供模型显示命令不等于已有稳定的无副作用机器目录接口；没有接入 `models` 的适配不会显示模型下拉。会话 ID 的观察记录不构成持久化与目录校验证明，不开放绑定或续聊。

## Cline

使用当前 [Cline CLI JSONL](https://github.com/cline/cline/blob/main/docs/cli/cli-reference.mdx#json-output-for-agent-runs)，通过标准输入传递指令。解析公开消息、按 `toolCallId` 关联的工具输入、进度与结果，保留源事件时间；不展示隐藏推理。`agent_event.done` 不是最终报告，必须收到 `run_result` 且 `finishReason: completed`；停止、失败或原生非零退出不发布局部答案。

模型、Provider、推理强度按原生参数传入；不修改登录配置或凭据。当前原生一次性模式默认自动批准工具，Wakeit 不额外扩大权限；请只在准备好的执行目录和原生策略下使用。原生会话和模型目录未声明。合成事件与来源见 [Cline fixture](../crates/wakeit/tests/fixtures/cline-provenance.md)。

## Goose

使用 [AAIF / Block Goose 的官方 CLI](https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/goose-cli-commands.md)：`goose run --instructions - --output-format stream-json`。从标准输入接收指令，转换真实消息、工具调用及结果；需要原生完成记录及成功退出才能生成最终报告，不与其他同名产品混用。Provider 和模型可手填，不读取认证配置，也不通过创建会话猜测模型目录。工具授权沿用原生无人值守策略。

## 扩展优先级

2026-10-16 核对公开仓库关注度后，这一批只新增 Cursor CLI、GitHub Copilot CLI、Cline、Goose、Qwen Code。开源候选的公开参考值为 [Cline](https://github.com/cline/cline)约 7.0 万星、[Goose](https://github.com/aaif-goose/goose)约 5.5 万星、[Qwen Code](https://github.com/QwenLM/qwen-code)约 2.9 万星；同时考虑维护活跃度与独立 CLI 的适用性。Cursor 和 Copilot 的闭源产品使用面单独判断，不与仓库星数直接混排，也不把平台、SDK 或 IDE 仓库的全部关注度视为 CLI 用户数。

Aider、Continue、OpenHands、Crush、Kilo、Droid、Amp、Vibe、Auggie、Qoder、Kiro 本轮只保留调研记录，不新增模板。没有非交互 CLI 的 IDE 产品也不通过编辑器启动器伪装为无人值守适配。

新增的原生协议 fixtures 均为按官方结构编写的合成数据，不含用户会话或凭据。Kimi 已核对本机 2.1.1 的帮助、版本及只读会话查询；其它新增 CLI 使用协议及进程桥接验证，不声称真实模型调用已通过。任务编辑器只列出所选 Runner 检测到的 Agent；Runner 的 Agents 页仍显示完整检测状态。已有任务绑定不会因检测状态变化而自动替换。
