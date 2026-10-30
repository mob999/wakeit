# macOS 系统部署

Gateway/NATS 使用 /Library/LaunchDaemons，不用登录后才加载的 LaunchAgents。运行账号为真实非 root 用户（示例当前用户名），数据库也不能以 root 运行。首次安装系统服务需要管理员操作。

可通过 Homebrew 安装 PostgreSQL 17、NATS Server；检测 brew --prefix 以确定实际路径。Gateway 放 /usr/local/bin/wakeit-gateway，配置和身份放 `/Library/Application Support/Wakeit`，避免受隐私保护的 Documents/Desktop 路径。

```sh
brew install postgresql@17 nats-server
sudo install -d -m 750 -o "$(id -un)" -g "$(id -gn)" '/Library/Application Support/Wakeit'
```

准备 gateway.yaml 和权限 600 的 gateway.env，初始化 NATS 身份，创建专用数据库和角色，按 deploy.md migrate/bootstrap。已有数据库直接复用，不重复 initdb。

## PostgreSQL 开机服务

Homebrew 用户服务不等于系统级数据库自启。新部署可在常规用户下 initdb 专用数据目录，然后为 postgres 的实际二进制及数据目录生成下面的系统 plist（替换三个占位值）：

```xml
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>Label</key><string>dev.wakeit.postgresql</string>
<key>UserName</key><string>ACTUAL_USER</string>
<key>ProgramArguments</key><array>
<string>ABSOLUTE_POSTGRES_BINARY</string><string>-D</string><string>ABSOLUTE_PG_DATA</string>
</array>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
<key>ThrottleInterval</key><integer>5</integer>
<key>StandardOutPath</key><string>/Library/Logs/Wakeit/postgresql.log</string>
<key>StandardErrorPath</key><string>/Library/Logs/Wakeit/postgresql.error.log</string>
</dict></plist>
```

使用 plistlib 生成实际文件，避免路径/XML 转义错误；以 root:wheel、644 安装到 /Library/LaunchDaemons/dev.wakeit.postgresql.plist。日志目录必须可写。确认没有另一 PostgreSQL 占用同端口后，以管理员执行 launchctl bootstrap system 加载，并验证数据库连接；若已有系统级 PostgreSQL 则复用其服务名。

## Gateway 与 NATS

```sh
wakeit-gateway service --config '/Library/Application Support/Wakeit/gateway.yaml' \
  --env-file '/Library/Application Support/Wakeit/gateway.env' --user "$(id -un)" \
  --platform macos --nats-bin "$(command -v nats-server)" \
  --nats-config '/Library/Application Support/Wakeit/nats/nats.conf' \
  --output '/Library/Application Support/Wakeit/services'
sudo '/Library/Application Support/Wakeit/services/install.sh'
```

生成的服务加载到 system 域并按指定用户运行，失败后重新启动；日志位于 /Library/Logs/Wakeit。依赖尚未就绪时 Gateway 失败重启，直到数据库和 NATS 可连接。

```sh
sudo launchctl print system/dev.wakeit.gateway
sudo launchctl print system/dev.wakeit.nats
sudo launchctl print system/dev.wakeit.postgresql
curl -fsS http://127.0.0.1:8080/health/ready
```

停止用 launchctl bootout system/dev.wakeit.gateway，重新加载用 bootstrap system 与 plist 绝对路径；生成的 uninstall.sh 停止并移除 Gateway/NATS plist，保留数据。数据库由自己的系统服务管理。

这是操作系统启动后的无人登录服务；FileVault 磁盘尚未解锁、系统未启动或机器休眠时不能运行。不要为部署删除 Agent 凭据；Runner 另按用户身份设置。
