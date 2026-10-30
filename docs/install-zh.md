# 安装 Wakeit CLI

[English](install.md)

macOS 或使用 GNU libc 的 Linux 运行：

```sh
curl -fsSL https://raw.githubusercontent.com/mob999/wakeit/main/install.sh | sh
```

CLI 包含 Runner 和官方 Agent 插件，与 Gateway 独立发布。脚本自动识别系统与架构，默认下载 CLI 0.2.0（或 `--version` 指定的版本），对照发布清单和校验列表验证 SHA-256，然后安装到 `~/.local/bin`。各阶段和下载过程均显示进度。脚本为当前用户的 Shell 配置 PATH；完成后打开新终端，运行 `wakeit --version`。无需 sudo。已有配置、服务进程、设备身份和 Agent 凭据均保留。

指定版本或安装目录：

```sh
curl -fsSL https://raw.githubusercontent.com/mob999/wakeit/main/install.sh | sh -s -- --version 0.2.0 --bin-dir "$HOME/.local/bin"
```

自行管理 PATH 时添加 `--no-modify-path`。脚本支持 zsh、bash、fish 和 POSIX Shell 的启动文件；其它 Shell 会提示需要加入 PATH 的目录。安装脚本不安装或更新 Gateway，也不会重启运行中的 Runner；升级服务请继续参照[更新指南](../skills/wakeit/references/updates.md)。

从最新正式 CLI Release 安装或更新完整 Wakeit Skill：

```sh
wakeit skill install
```

默认安装到 `~/.agents/skills/wakeit`。用 `--dir` 指定 Agent 使用的最终 Skill 目录：

```sh
wakeit skill install --dir /path/to/agent/skills/wakeit
```

该命令支持 macOS、Linux 和 Windows，无需登录 Gateway。校验压缩包后替换目标目录内的 Wakeit Skill，不影响其他技能。再次执行即可更新，完成后按需重新加载 Agent。添加 `--json` 可获取机器可读结果。

CLI 发布产物也会提供 `install.sh` 及其 `SHA256SUMS`，可以先下载、查看脚本再执行。

## 手工安装与 Windows

Windows 或需要手工安装时，进入[官方发布页](https://github.com/mob999/wakeit/releases)，选择 `cli-v*` 版本，下载 `wakeit-cli-release.json`、`SHA256SUMS` 和对应平台的压缩包。将压缩包的 SHA-256 与两份发布文件核对后，解压到持久的用户 bin 目录，并将该目录加入 PATH。保留可执行文件旁的 `wakeit-plugins` 目录。

| 系统 | 产物 |
| --- | --- |
| macOS Apple Silicon | `wakeit-aarch64-apple-darwin.tar.gz` |
| Linux GNU x86-64 | `wakeit-x86_64-unknown-linux-gnu.tar.gz` |
| Linux GNU ARM64 | `wakeit-aarch64-unknown-linux-gnu.tar.gz` |
| Windows x86-64 | `wakeit-x86_64-pc-windows-msvc.tar.gz` |

Windows 使用 `Get-FileHash -Algorithm SHA256` 计算校验值，使用 `tar -xzf wakeit-x86_64-pc-windows-msvc.tar.gz -C C:\path\to\bin` 解压已验证的压缩包，并将该目录加入当前用户的 PATH。保留其中的 `wakeit.exe` 和 `wakeit-plugins`。在新 Shell 中验证 `wakeit --version`。不要在原生 PowerShell 中运行 Unix Shell 安装脚本。

校验值或版本不匹配时，脚本会在替换现有可执行文件前停止。处理提示的问题后重跑相同命令即可；下载文件只是临时文件，不会删除服务数据。

自行托管服务请使用独立的 [Gateway 部署指南](gateway-service.md)。不要将 Gateway 版本号传给 CLI 安装脚本。

CLI 0.2 调整了适配器安装方式，参见[已有 Runner 配置升级](plugins-zh.md#从-cli-01-升级)。
