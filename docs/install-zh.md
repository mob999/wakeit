# 安装 Wakeit

[English](install.md)

macOS 或使用 GNU libc 的 Linux 运行：

```sh
curl -fsSL https://raw.githubusercontent.com/mob999/wakeit/main/install.sh | sh
```

脚本自动识别系统与架构，下载当前稳定版本，对照发布清单和校验列表验证 SHA-256，然后安装到 `~/.local/bin`。各阶段和下载过程均显示进度。脚本为当前用户的 Shell 配置 PATH；完成后打开新终端，运行 `wakeit --version`。无需 sudo。已有配置、服务进程、设备身份和 Agent 凭据均保留。

指定版本或安装目录：

```sh
curl -fsSL https://raw.githubusercontent.com/mob999/wakeit/main/install.sh | sh -s -- --version 0.3.3 --bin-dir "$HOME/.local/bin"
```

自行管理 PATH 时添加 `--no-modify-path`。脚本支持 zsh、bash、fish 和 POSIX Shell 的启动文件；其它 Shell 会提示需要加入 PATH 的目录。安装脚本不会重启运行中的 Gateway 或 Runner；升级服务请继续参照[更新指南](../skills/wakeit/references/updates.md)。

Agent 可以添加 `--skill-dir /path/to/agent/skills/wakeit`，同时安装匹配版本的完整 Wakeit skill。应使用该 Agent 实际配置的技能目录。已有内容完全相同时直接复用；存在差异时保留原文件并报告，不直接覆盖。Agent 若需要重新加载才能发现技能，应在安装后重新加载。

只安装 Skill，不下载或替换 CLI，也不修改 PATH：

```sh
curl -fsSL https://raw.githubusercontent.com/mob999/wakeit/main/install.sh | sh -s -- --version 0.3.3 --skill-only --skill-dir "/absolute/path/to/agent/skills/wakeit"
```

后续发布产物也会提供 `install.sh` 及其 `SHA256SUMS`，可以先下载、查看脚本再执行。

## 手工安装与 Windows

Windows 或需要手工安装时，进入[官方发布页](https://github.com/mob999/wakeit/releases)，下载 `wakeit-release.json`、`SHA256SUMS` 和对应平台的二进制。将二进制的 SHA-256 与两份发布文件核对后，放入持久的用户 bin 目录，并将该目录加入 PATH。

| 系统 | 产物 |
| --- | --- |
| macOS Intel | `wakeit-x86_64-apple-darwin` |
| macOS Apple Silicon | `wakeit-aarch64-apple-darwin` |
| Linux GNU x86-64 | `wakeit-x86_64-unknown-linux-gnu` |
| Linux GNU ARM64 | `wakeit-aarch64-unknown-linux-gnu` |
| Windows x86-64 | `wakeit-x86_64-pc-windows-msvc.exe` |

Windows 使用 `Get-FileHash -Algorithm SHA256` 计算校验值，将验证后的文件重命名为 `wakeit.exe`，并将所在目录加入当前用户的 PATH。在新 Shell 中验证 `wakeit --version`。不要在原生 PowerShell 中运行 Unix Shell 安装脚本。

校验值或版本不匹配时，脚本会在替换现有可执行文件前停止。处理提示的问题后重跑相同命令即可；下载文件只是临时文件，不会删除服务数据。
