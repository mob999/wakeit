# 第三方组件

## README Crafter

- 上游：https://github.com/linhai0872/readme-crafter-skill
- 固定提交：`08db7782836c542d9b597860da33fcc159e36433`
- 仓库位置：`.agents/skills/readme-crafter-skill/`
- 授权：MIT，保留目录中的上游 LICENSE。
- 本地调整：将 compatibility 前置字段移入 metadata，保留原内容以适配当前技能校验器。

按上游指南分类为 source-first、面向新用户的开发者工具；README 先提供真实安装和首个任务流程，再引导到部署、CLI 和协议文档。

## WinSW

Windows Gateway/NATS 系统服务使用 WinSW 2.12.0 x64。安装脚本从官方 Release 下载工具，校验 SHA-256 后使用；二进制不随本仓库提交。

- 上游：https://github.com/winsw/winsw
- 下载：https://github.com/winsw/winsw/releases/download/v2.12.0/WinSW-x64.exe
- SHA-256：`05b82d46ad331cc16bdc00de5c6332c1ef818df8ceefcd49c726553209b3a0da`
- 授权：MIT，见上游对应版本的 LICENSE。

Rust 依赖的版本固定在 Cargo.lock；`self-replace` 用于跨平台替换当前二进制，`semver` 用于版本比较，`dotenvy` 用于读取服务环境文件。
