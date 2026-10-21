# 连接现有服务

已有明确 Gateway 地址时直接接入，不部署另一套。先请求 `/health/ready` 和 `/api/v1/meta`，地址不可达就向用户确认可访问的地址。公网要求 HTTPS，loopback 可用 HTTP。

先取得目标工作区 ID，检查 `wakeit setup --help` 是否支持 `--workspace` 和实际安装版本。缺少 CLI 或接入能力时，使用 Gateway `/api/v1/meta` 中 `install` 指定的官方版本及下载地址；当前正式版为 [v0.3.1](https://github.com/mob999/wakeit/releases/tag/v0.3.1)。下载对应版本的 `wakeit-release.json`、`SHA256SUMS` 及对应平台原生二进制，校验 SHA-256 同时匹配两份清单，再安装到用户持久 bin 目录。若官方安装包缺少 `--workspace`，停止并报告与当前 Gateway 不兼容，不能改用无工作区命令或自行编译替代。保留现有可执行文件直到新文件校验成功，采用原子替换。支持 x86_64/aarch64 Linux GNU、x86_64/aarch64 macOS、x86_64 Windows。

```sh
wakeit setup --gateway https://wakeit.example.com --workspace WORKSPACE_ID
# 可选：--runner-name dev-server --allow-root /absolute/project
# 仅连接 CLI：
wakeit login --gateway https://wakeit.example.com --workspace WORKSPACE_ID
wakeit connection list
wakeit connection use <name>
```

`setup` 默认设备名为主机名，执行范围为当前工作目录。打开浏览器后由用户核对验证码、设备和目录并明确授权。Agent 不读取用户密码，不代替用户点击授权。浏览器打不开时展示验证链接和用户验证码，等待用户操作；不输出设备码或 API Key。

setup 保存连接和设备身份、导出全部内置适配、安装当前用户自启并验证在线及适配检测。已有配置优先复用，不扩大执行目录、不重建设备、不安装 Agent、不改 Agent 登录或模型。步骤失败后保留完成部分，修正具体问题后重跑相同命令。无 systemd 用户管理器时不擅自安装 root 服务。

连接按 Gateway、工作区、账号隔离，API Key 固定一个工作区，在平台标准配置目录私密存储，后续命令跨目录可用。显式 `--gateway`、`--token`、`--token-file` 及对应 WAKEIT 环境变量优先；切换地址不会把另一连接的 Key 发过去。保留显式用户名的密码登录供已有脚本使用；密码由用户在本地提供，不粘贴到聊天。

安装 CLI 时同时配置持久的用户 PATH。Unix 按 `$SHELL` 识别当前用户实际使用的 shell，未设置时才回退到账户登录 shell，并更新该 shell 对应的启动文件；不能因为是 macOS 就假定使用 zsh。Windows 保留已有用户 PATH，并在刷新环境后的新 shell 进程中验证。从其他目录启动匹配的新登录 shell，直接执行 `wakeit --version` 和 `wakeit connection list`；绝对可执行文件路径、临时 export 或换用另一种 shell 都不能证明 PATH 已正确配置。

完成后从其他工作目录检查 `wakeit connection list`、`wakeit runner list`、`wakeit agent list`、`wakeit task list`，确认 Runner 在线、范围正确、自启已安装，并报告实际检测结果。不自动创建或运行任务。

Admin、Operator 可注册 Runner，只有 Admin 可吊销；Viewer 可单独登录 CLI。API Key 默认不过期，Settings → API Key 可撤销。设备身份不能代替个人凭据。
