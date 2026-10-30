# 内置 Agent 适配

Runner 调用设备上已安装、已登录的原生 CLI，不安装 Agent、不复制模型凭据。所有模板与第三方适配使用同一配置及事件协议。

| ID | 原生命令 | 实时过程 | 模型选择 | 会话绑定与续聊 |
|---|---|---|---|---|
| `codex` | `codex` | 支持 | 原生查询或手填 | 支持 |
| `claude-code` | `claude` | 支持 | 手填 | 支持 |
| `pi` | `pi` | 支持 | 原生查询或手填 | 支持 |
| `opencode` | `opencode` | 工具完成后更新 | 原生查询或手填 | 支持 |
| `kimi-code` | `kimi`，Kimi Code 2.x | 支持 | 手填 | 只读查询原生会话、精确绑定及原生命令续聊 |
| `agy` | Google Antigravity CLI 的 `agy` | 支持 | 原生 `models` 查询或手填 | 不支持 |
| `gemini-cli` | `gemini` | 支持 | 手填 | 不支持 |
| `cursor` | `cursor-agent` | 支持 | 手填 | 不支持 |
| `copilot` | `copilot` | 支持 | 手填 | 不支持 |
| `cline` | `cline` | 支持 | 手填 | 不支持 |
| `goose` | `goose`（AAIF / Block） | 支持 | 手填 | 不支持 |
| `qwen-code` | `qwen` | 支持 | 手填 | 不支持 |

没有模型接口时只显示输入框；查询失败或空列表也不会用静态目录补齐。模型 ID、推理强度和原生权限由实际 CLI 决定，配置目录不是账号调用权限的保证。

## 导出和启用

`wakeit setup`／`runner enroll` 默认直接加载官方插件，不复制 YAML。插件变更在 Runner 下次启动生效。

需要单独定制时，用 `agent init` 导出所需配置，将路径加入 `runner.yaml.agents`。若与官方资源使用同一个 ID，可将 `bundled_agents` 设为 `false` 并只列出需要的 YAML；也可给定制配置使用不同的 ID。普通使用无需导出：

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
wakeit plugin exec git:github.com/mob999/wakeit adapter -- detect kimi-code
wakeit plugin exec git:github.com/mob999/wakeit adapter -- detect agy
wakeit plugin exec git:github.com/mob999/wakeit adapter -- detect gemini-cli
```

这些探测只运行原生 `--help` / `--version`，不会启动模型会话。Kimi 要求当前 TypeScript Kimi Code 2.x，不把已归档的 Python `kimi-cli` 视为同一协议；Antigravity 探测也会拒绝缺少 headless 参数的同名桌面启动器。

## Kimi Code

使用 `wakeit plugin exec git:github.com/mob999/wakeit adapter -- kimi` 驱动已安装的 Kimi Code 2.x。

辅助命令为每次执行启动一个仅监听 `127.0.0.1` 的 `kimi web --no-open` 子进程，通过[官方本地 Server API](https://www.kimi.com/code/docs/en/kimi-code-cli/reference/server-api.html)订阅真实 `assistant.delta`、`tool.call.started`、`tool.result` 和 `turn.ended`。先确认订阅成功再提交一次指令，并处理原生心跳；绑定已有会话先通过空的会话级配置调用加载原生上下文，不覆盖已有配置。消息以完整快照更新，工具输出不混入最终报告；只有原生 `completed` 及正常进程退出才发布报告。子进程与 Runner 共用进程树，取消或失败时结束本次执行及其本地服务，不连接用户自行启动的 Kimi 服务，也不打开浏览器。

大型工具参数、工具输出及消息只截断 Wakeit 保留的过程预览，不中止原生执行；工具状态和原生结束状态保持真实。最终报告超出保留容量时使用 `truncated: true` 标记，下游 DAG 不会将不完整报告当作完整输入。Trace 缓存和重复事件记录保持有界；超过明细保留容量后仍读取原生完成事件。

新建会话使用原生 API 的 `title` 字段保存“任务名-wakeit-时间戳”；绑定已有会话保持原标题。标题与上下文由 Kimi 自己持久化，不直接改写其状态文件。服务地址来自与子进程 PID 匹配的本地登记，认证使用其私密 `server.token`，不会写入 Wakeit 事件、日志、命令行或 URL。API 密钥和 Agent 登录凭据不读取、不复制、不修改。

会话发现仍只读 `KIMI_CODE_HOME`（默认 `.kimi-code`）中的原生索引及匹配工作目录的 `state.json`，精确校验完整 ID、真实目录和持久化状态。模型沿用本地默认值或任务显式参数；绑定时未覆盖模型则保留原生绑定，不增加静态模型目录。协议来源与真实验收范围见 [Kimi 验证记录](../crates/wakeit-agent-runtime/tests/fixtures/kimi-code/README.md)。

## Antigravity CLI（agy）

依据 [Google 官方 headless 文档](https://antigravity.google/docs/cli/headless/)：Wakeit 将指令编码为单个 JSON stdin 事件，使用 `--input-format stream-json --output-format stream-json --disable-slash-commands`，保留中文、多行和引号，不把指令内容解释为 slash command。stdout 的 `init`、`step_update`、`result` 转换为会话元数据、公开过程与最终报告，工具输出不冒充报告。

模型查询使用 `agy --output-format json models`，只解析实际返回的目录；全局参数在子命令之前，见[上游确认](https://github.com/google-antigravity/antigravity-cli/issues/777)。模板不设置跳过权限的参数，沿用原生权限策略；需要确认而未获授权的工具可能被原生拒绝，用户应按 Agent 的实际配置处理。

虽然原生支持 `--conversation ID`，适配尚未接入精确 ID、目录和持久化检查，因此不提供 Wakeit 的会话绑定或续聊入口，也不把 `init` 事件直接当作已持久化证明。未打包 Antigravity 二进制，其使用遵循 Google 原生条款。

## Gemini CLI

使用[官方 headless 流式输出](https://geminicli.com/docs/cli/headless/)，通过标准输入传递指令，并显式设置 `--prompt '' --output-format stream-json`。工具执行遵循已安装 CLI 的权限策略，不添加 `--yolo`。消息增量合并成完整快照，工具与命令分开显示；只有原生成功终态对应的最终消息作为报告。缺少终态、原生失败或输出中断不伪报成功。

当前适配提供手填模型及独立执行，不提供模型目录或会话绑定。Google 已宣布 [Gemini CLI 向 Antigravity CLI 过渡](https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli)；此模板面向仍在使用 `gemini` 的设备，不假设两者的事件格式兼容。

## Cursor、GitHub Copilot CLI 与 Qwen Code

[Cursor 官方输出协议](https://cursor.com/docs/cli/reference/output-format)提供完整消息、工具开始与完成、最终 `result`；适配使用 `cursor-agent --print --output-format stream-json`，按稳定工具 ID 更新过程。官方安装器仍提供 `cursor-agent` 别名，避免与编辑器的 `cursor` 命令混淆。不添加 `--force`：按照[原生 headless 约定](https://cursor.com/docs/cli/headless)，默认不强制应用文件修改；这不等同操作系统只读沙箱，需要写入的用户应明确配置原生权限。

[GitHub Copilot CLI](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-programmatic-reference)使用原始标准输入传递指令，`--no-ask-user --output-format json` 返回原生 JSONL。按实际 CLI 1.0.98 输出解析公开消息、工具开始与完成；只在成功的原生结果记录和进程退出后发布一次最终报告，不读取隐藏推理。它不是旧版 `gh copilot` 扩展，也不套用 SDK 或 workflow 的事件格式。保留原生工具权限，不默认添加允许所有工具或扩大目录范围的参数；无人值守时需要的权限应事先明确配置。

[Qwen Code headless 协议](https://github.com/QwenLM/qwen-code/blob/main/docs/users/features/headless.md)的完整 assistant 消息、tool_use / tool_result 和最终 result 使用独立适配解析，不沿用 Gemini 的事件格式。只有成功终态提供最终报告；工具输出、未完成消息和隐藏推理不冒充报告。

三者模型手填。原生 CLI 提供模型显示命令不等于已有稳定的无副作用机器目录接口；没有接入 `models` 的适配不会显示模型下拉。会话 ID 的观察记录不构成持久化与目录校验证明，不开放绑定或续聊。

过程展示预算不决定执行成败：Copilot 的大消息、工具输入输出使用截断预览；Copilot、Cursor、Qwen Code 达到过程数量或待处理内容预算后停止保留后续过程，继续读取原生终态。界面会标记过程不完整，任务仍按原生执行结果收尾。最终报告超出传输容量时保留带截断标识的预览；DAG 下游不得将其当作完整结果使用。原生失败、缺少终态和真实协议错误仍会失败。

## Cline

使用当前 [Cline CLI JSONL](https://github.com/cline/cline/blob/main/docs/cli/cli-reference.mdx#json-output-for-agent-runs)，通过 `--` 后的独立 argv 传递指令，保留多行、中文及 shell 字面量；当前 3.0.72 的管道输入不能可靠启动任务。解析公开消息、按 `toolCallId` 关联的工具输入、进度与结果，保留源事件时间；不展示隐藏推理。`agent_event.done` 不是最终报告，必须收到 `run_result` 且 `finishReason: completed`；停止、失败或原生非零退出不发布局部答案。

模型、Provider、推理强度按原生参数传入；不修改登录配置或凭据。当前原生一次性模式默认自动批准工具，Wakeit 不额外扩大权限；请只在准备好的执行目录和原生策略下使用。原生会话和模型目录未声明。合成事件与来源见 [Cline fixture](../crates/wakeit-agent-runtime/tests/fixtures/cline-provenance.md)。

## Goose

使用 [AAIF / Block Goose 的官方 CLI](https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/goose-cli-commands.md)：`goose run --instructions - --output-format stream-json`。从标准输入接收指令，转换真实消息、工具调用及结果；需要原生完成记录及成功退出才能生成最终报告，不与其他同名产品混用。Provider 和模型可手填，不读取认证配置，也不通过创建会话猜测模型目录。工具授权沿用原生无人值守策略。

## 设备检测

任务编辑器只列出所选 Runner 检测到的 Agent；设备详情仍显示完整检测状态。已有任务不会因检测变化而自动替换 Agent。检测成功只代表原生命令可用，实际调用仍取决于 Agent 的登录、供应商和工具权限配置。
