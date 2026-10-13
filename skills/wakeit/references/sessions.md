# 任务会话与本机续聊

先区分用户意图：绑定已有原生会话，使后续 Task 执行追加到同一上下文；或打开一个已结束 Run 的会话，让用户继续交互。不要把续聊当作重新触发或 retry。

## 任务绑定

读取目标 Runner 上实际适配能力。只支持声明会话能力的同步适配；未支持或没有可靠原生 ID 时不要编造，也不要使用 `--last` 或“最近会话”。默认不绑定，每个 Run 新建独立持久会话。新原生标题由 Wakeit 生成 `任务名-wakeit-YYYYMMDD-HHMMSSmmm`，使用工作区时区和实际开始时间并持久化；不要在 Task、parameters 中另加 title，不自行重算或回填旧会话标题。总长最多 200 UTF-8 字节，过长只截短任务名部分。

```yaml
session:
  id: actual_native_session_id
```

适配上报 session_list_supported 时，先在所选 cwd 查询实际列表；没有列表接口时可使用用户提供的明确 ID，并通过 inspect 核对。不要从其他目录、静态别名或最新文件猜候选。

选择列表只提供标题与 ID，不显示 query 或首条指令预览。标题不能代替唯一 ID。绑定已有会话保留其原生标题，修改 Task 名称不应修改它。

使用实际 Session ID，不是 Run ID、Task ID 或任意文件路径。绑定只适用于原 Runner、Agent 和工作目录；更换这些字段时重新核对。配置保存后，每次触发都继续同一上下文，历史会累积。会话不存在时失败，禁止退回新建；Gateway 不保存原生聊天历史。

## Run 续聊

从 `wakeit run get <run-id>` 读取实际 session 信息；确认 Run 已结束、session.ready 为 true 且 command 非空后再提供原生命令。运行中或 unknown 不提供续聊入口。WebUI 复制的是 `session.command` 中的原生 Agent 续聊命令，含执行目录。使用已保存的命令，不自行拼 shell、不把 ID 替换成 last/continue。macOS/Linux 使用 POSIX shell，Windows 使用 PowerShell，在原 Runner 同一用户的真实终端运行。

原生命令直接启动 Agent，不经过 Wakeit 会话检查或锁；开始前结束其他终端和 Wakeit 中使用同一会话的运行。会话已删除时，不宣称原生 CLI 一定会拒绝新建。没有可导出 command 时，不编造命令或改抄日志；部分适配依赖额外环境，无法安全导出。

希望使用本地验证和互斥时，可提供可选入口：

```sh
wakeit run resume <run-id>
```

该 CLI 使用本地登记配置及 Journal 里的适配快照，校验设备、目录范围、原生会话和锁，不需要 Gateway 在线或 API Key。无法发现本地配置时用 `--runner-config /absolute/path/runner.yaml`；不要要求用户打印设备凭据。这个入口与默认复制原生命令分开，不擅自将用户要求的原生命令替换为 Wakeit 命令。

只要求复制时不要实际启动交互程序。若用户要求代为续聊，只有能承接输入的真实终端才启动；会话 ID 不可靠、未持久化、记录已清理、设备不符或本地校验失败时解释实际错误，不换到别的会话。

`runner enroll`/`runner start` 自动登记当前用户的配置路径。登记表不复制凭据，不能将另一台设备的路径注册成本机会话。

## 占用与不确定状态

同一会话的 Wakeit 运行及 `wakeit run resume` 共享本地文件锁；被占用的新执行跳过，即便任务允许重叠。unknown 保持占用，先核对原进程实际结束再 resolve。Runner 重启、心跳过期或没有收到 session 事件均不构成重新提交授权。

直接原生 CLI 启动不受 Wakeit 锁控制；要先结束其他终端中的同一会话。取消只终止当前执行，不删除原生会话。重试绑定任务可能再次产生副作用，并继续原绑定；独立任务重试保持独立，使用新的原生会话及新的开始时间标题。

交互 CLI 被强制结束或无法确认进程树退出时，本地占用标记会保留。先核对该原生 Agent 及其子进程均已停止，再由用户明确清理错误消息指向的单个标记文件。不要自动删除、使用通配符批量清理，或修改历史 Run 状态来绕过占用保护。
