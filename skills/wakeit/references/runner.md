# Runner

Runner 主动连接 NATS，不需设备开放入站端口。Gateway 对外地址与 NATS/WSS 地址必须能从该设备访问；本机 localhost 不能直接用于另一台设备。

Admin 或 Operator 可用 `wakeit setup --gateway URL` 完成浏览器授权、注册和用户自启。手动流程由 Admin 或 Operator 生成一次性注册令牌：`wakeit runner enrollment`，或 WebUI Runners 的连接设备操作。输出含敏感令牌，直接使用本机文件/环境变量承接，不复制到聊天或日志。令牌 30 分钟内有效、只能用一次。

```sh
wakeit runner enroll --name dev-server --token-env WAKEIT_ENROLL_TOKEN \
  --dir /absolute/path/runner --allow-root /absolute/path/projects
wakeit agent doctor --config /absolute/path/runner/runner.yaml
wakeit runner start --config /absolute/path/runner/runner.yaml
```

可重复 --allow-root；它是用户批准的执行范围，不默认开放全盘。注册导出全部内置 YAML，可修改 runner.yaml 的 agents 列表。安装和登录 Agent 由 Runner 的真实运行用户完成，模型凭据保留本机。

自启命令：

```sh
wakeit runner service --config /absolute/path/runner.yaml --platform linux --output wakeit-runner.service
```

Linux 将单位放入该用户 ~/.config/systemd/user/，执行 systemctl --user enable --now wakeit-runner.service；需要退出登录后执行则管理员 enable-linger 该用户。macOS platform=macos 输出 LaunchAgent plist，放到 ~/Library/LaunchAgents，通过 launchctl bootstrap gui/<uid> 加载。Windows platform=windows 输出 PowerShell 登录计划任务脚本，在 Agent 已授权用户下运行。

Runner 默认用户登录自启，不与 Gateway 系统服务混用身份。Windows 无人登录执行需要专用服务账号及该账号的 Agent 登录配置，不能声称默认登录计划任务已经支持登录前执行。

后台 PATH 与终端可能不同，Agent submit/version/models 命令尽量用实际绝对路径。休眠、断网和关机不会被自启配置消除。吊销用 `wakeit runner revoke <id>`，会影响关联任务；只在用户要移除设备时执行。
