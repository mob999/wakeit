# Windows 系统部署

使用管理员 PowerShell。Gateway/NATS 注册为 Automatic 的 Windows 系统服务，不用 AtLogOn 计划任务。WinSW 2.12.0 x64 在安装时从官方 Release 下载并校验固定 SHA-256。

将 wakeit-gateway.exe 和 nats-server.exe 放入持久安装目录（例如 C:\Program Files\Wakeit），数据和配置放 C:\ProgramData\Wakeit。安装 PostgreSQL 17+ 的 Windows 发行包，其数据库服务应设为 Automatic，并创建专用角色和数据库。查看 Get-Service 得到实际名称，例如 postgresql-x64-17。

Gateway/NATS 默认建议使用 LocalService；支持 NetworkService、LocalSystem，但不把 Runner/Agent 的登录身份换成这些账号。环境文件中保存数据库 URL，避免用户终端环境变量在开机时缺失。

```powershell
wakeit-gateway nats-init --dir C:\ProgramData\Wakeit\nats
wakeit-gateway migrate --config C:\ProgramData\Wakeit\gateway.yaml --env-file C:\ProgramData\Wakeit\gateway.env
wakeit-gateway bootstrap --config C:\ProgramData\Wakeit\gateway.yaml --env-file C:\ProgramData\Wakeit\gateway.env --username admin
wakeit-gateway service --config C:\ProgramData\Wakeit\gateway.yaml `
  --env-file C:\ProgramData\Wakeit\gateway.env --user LocalService --platform windows `
  --nats-bin 'C:\Program Files\Wakeit\nats-server.exe' `
  --nats-config C:\ProgramData\Wakeit\nats\nats.conf `
  --postgres-service postgresql-x64-17 --output C:\ProgramData\Wakeit\services
```

bootstrap 密码由 $env:WAKEIT_PASSWORD 提供，用后 Remove-Item Env:WAKEIT_PASSWORD。初次初始化 NATS 时可先前台启动依赖，初始化完成后停止前台实例，避免端口冲突。

授予 LocalService 对配置和环境文件只读、NATS 身份及 JetStream 数据目录所需读写；移除普通 Users 对身份/环境文件的读取权限。不要让服务使用用户 Documents 数据路径。生成的安装脚本会限制 gateway.env 权限，其他数据目录权限由部署步骤设置。

```powershell
Set-Service postgresql-x64-17 -StartupType Automatic
Start-Service postgresql-x64-17
& C:\ProgramData\Wakeit\services\install.ps1
Get-Service wakeit-gateway,wakeit-nats,postgresql-x64-17
Get-CimInstance Win32_Service -Filter "Name='wakeit-gateway'" | Select-Object Name,StartMode,StartName
Invoke-RestMethod http://127.0.0.1:8080/health/ready
```

安装脚本是显式管理员操作；不用自行降低系统执行策略。如果策略阻止脚本执行，按组织允许的脚本签名/执行流程处理。

查看 services 目录里的 WinSW 日志；停止用 Stop-Service wakeit-gateway，卸载运行生成的 uninstall.ps1。安装目录必须保留 wrapper 和 XML，卸载不删除数据库和 NATS 数据。外部 PostgreSQL/NATS 则验证其持续可用，省略不属于本机的依赖服务选项。
