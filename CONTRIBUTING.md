# 开发 Wakeit

Wakeit 尚未发布。产品与包版本固定 0.1.0，配置和消息协议固定 version: 1；数据库只保留完整的 `0001_initial.sql`。开发约定见 [AGENTS.md](AGENTS.md)。

## 提交流程

先创建或关联 GitHub Issue，从最新 main 创建带类型与 scope 前缀的 feature branch（如 `feat(gateway)/device-auth`、`fix(runner)/reconnect`），不用 `codex/` 前缀。在分支上提交和推送，通过关联 Issue 的 Pull Request 交付。禁止 force push、改写已推送的提交或标签，以及直接向 main 推送代码。后续修正追加提交，保留审查历史。

## 构建与检查

需要 Rust 1.89+、Node.js 22.12+，文档及发布检查使用 Python 3.12；完整服务测试需要 PostgreSQL 16+ 和 NATS 2.15。

```sh
npm --prefix web ci
npm --prefix web run build
cargo build --locked
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

WebUI 资源嵌入 Rust 二进制，修改生产资源后需要重新构建 Rust。热重载见 [运维指南](docs/operations.md#webui-热重载)。

## 集成测试

```sh
cargo build --locked --examples
python3 scripts/integration.py --pg-bin /path/to/postgresql/bin --keep-running
```

浏览器回归不在 CI 中运行。需要本地验证界面时，保持该隔离测试栈运行，另一个终端执行：

```sh
cd web
npx playwright install chromium
npm test
```

测试凭据和端口保存在忽略目录 `.local/`，不得提交。测试栈与日常预览独立，退出测试时关闭临时服务。

## WebUI 翻译

界面使用 i18next / react-i18next，资源位于 `web/src/locales/zh.json` 和 `en.json`。保留现有文案含义，只翻译界面文字，不翻译任务指令、报告、日志、路径、模型 ID 或第三方返回的内容。新增文案需同时维护两种语言；整句使用插值，数量使用复数资源，不拼接不同语序的句子。模块级目录使用延迟读取，组件订阅语言变化，避免切换时重建页面或丢失草稿。

首次使用按浏览器语言选择中文或英文，之后使用浏览器保存的 `wakeit-language`。顶栏版本号右侧可切换，登录页也提供同一入口。复制的 Agent Prompt 独立于界面语言。

本地双语回归：`WAKEIT_TEST_URL=http://127.0.0.1:55426 WAKEIT_TEST_STACK=/tmp/wakeit-no-stack.json npm --prefix web test -- tests/languages.spec.ts tests/onboarding.spec.ts`。这些场景模拟 API，不使用真实账号、报告或设备；浏览器回归仍不进入 CI。

发布产物构建使用 `.github/workflows/release.yml` 的手动运行，默认只构建并上传 Actions artifacts。只有匹配包版本的正式标签才发布 Release。当前 `v0.1.0` 作为预览 Release 发布，普通更新检查仍忽略预览版。已推送标签不改写；后续发布通过 Issue、feature branch 和 PR 准备。

[README Crafter skill](.agents/skills/readme-crafter-skill/SKILL.md) 是维护 README 的仓库内技能；写完核对命令、配置、链接和真实发布状态。

main 的服务器保护要求通过 PR、分支保持最新、讨论解决，以及 `rust (ubuntu-latest)`、`rust (macos-latest)`、`rust (windows-latest)`、`integration` 四项 GitHub Actions 检查；禁止删除和强推，对管理员同样生效。不要求额外的外部审批人数，以支持单维护者通过合格 PR 合并。
