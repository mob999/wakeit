# Linux 系统部署

适用于 systemd 主机。优先检测现有 PostgreSQL、NATS 和同名服务，已有服务不重置数据。以下新部署示例采用专用 wakeit 账号及 /var/lib/wakeit。

安装 PostgreSQL 16+（发行版包或官方仓库），确认其系统服务开机启用；安装 NATS Server 2.15+ 到 /usr/local/bin/nats-server。将 wakeit-gateway 放在 /usr/local/bin/wakeit-gateway。不要把当前用户的临时构建目录当服务路径。

```sh
sudo useradd --system --home /var/lib/wakeit --create-home --shell /usr/sbin/nologin wakeit
sudo install -d -m 750 -o wakeit -g wakeit /var/lib/wakeit
```

仅在账号不存在时创建。复制 assets/gateway.yaml，nats_auth_dir: ./nats，数据和环境文件归 wakeit；gateway.env 用 chmod 600。数据库管理员创建专用 LOGIN 角色 wakeit 与其拥有的 wakeit 数据库，密码在本机受限文件或数据库交互会话设置，不放命令行。

以 wakeit 用户初始化 NATS 身份。按 deploy.md 执行 migrate/bootstrap（环境文件独立提供），然后生成：

```sh
wakeit-gateway service --config /var/lib/wakeit/gateway.yaml \
  --env-file /var/lib/wakeit/gateway.env --user wakeit --platform linux \
  --nats-bin /usr/local/bin/nats-server --nats-config /var/lib/wakeit/nats/nats.conf \
  --postgres-service postgresql.service --output /var/lib/wakeit/services
sudo systemctl enable --now postgresql.service
sudo /var/lib/wakeit/services/install.sh
```

postgres-service 根据实际发行版服务名调整。生成的单位有 multi-user.target、自启动、失败重启和 NATS/指定数据库依赖。初始化时若 NATS 尚未运行，可先手动启动，完成初始化后退出前台实例再安装服务，避免端口冲突。

```sh
systemctl is-enabled wakeit-gateway.service wakeit-nats.service
systemctl status wakeit-gateway.service wakeit-nats.service
journalctl -u wakeit-gateway.service -n 50
curl -fsS http://127.0.0.1:8080/health/ready
```

停止用 systemctl stop wakeit-gateway.service；卸载执行生成的 uninstall.sh，不删除持久数据。Compose 路径则启用 Docker 系统服务，复核整个 Compose 栈的重启策略，不另外运行同端口原生 Gateway。
