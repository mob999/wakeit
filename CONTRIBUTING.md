# 开发 Wakeit

Gateway 当前版本为 0.4.2，CLI（含 Runner）为 0.1.1，两者独立发布。配置和消息协议保持 version: 1；已发布的数据库迁移基线不可改写，后续结构变更通过新增迁移交付。开发约定见 [AGENTS.md](AGENTS.md)。

## 提交流程

先创建或关联 GitHub Issue，从最新 main 创建带类型与 scope 前缀的 feature branch（如 `feat(gateway)/device-auth`、`fix(runner)/reconnect`），不用 `codex/` 前缀。在分支上提交和推送，通过关联 Issue 的 Pull Request 交付。禁止 force push、改写已推送的提交或标签，以及直接向 main 推送代码。后续修正追加提交，保留审查历史。

## 构建与检查

CLI 构建只需 Rust 1.89+；Gateway 的 WebUI 构建另需 Node.js 22.12+。文档及发布检查使用 Python 3.12；完整服务测试需要 PostgreSQL 16+ 和 NATS 2.15。

以下是按需使用的命令参考，不是每次改动的本地必跑清单。全量测试、跨平台构建、服务集成和发布打包交给 CI；本地只运行能回答当前具体疑问的检查，已通过的检查不重复运行。仅在依赖或环境变化时重新安装依赖。

```sh
npm --prefix web ci
npm --prefix web run test:unit
npm --prefix web run build
cargo build --locked -p wakeit-cli
cargo build --locked -p wakeit-gateway
cargo fmt --all --check
cargo clippy --locked -p wakeit-cli --all-targets -- -D warnings
cargo test --locked -p wakeit-cli --all-targets
```

UI 开发默认由一个 Vite dev 服务直接提供当前工作树的 `web/`，保存后通过热重载观察相关页面和交互；你我使用同一预览地址，不在多份源码之间反复复制。普通前端修改不要求逐次构建或重启后端；生产发布时才将构建资源嵌入 Rust 二进制。启动命令见下方“WebUI 热重载”。

实现或准备 PR 的任务完成必要的局部验证后即可推送，让 CI 异步运行并继续开发；合并和发布时等待当前提交的必需 CI。只在结果、具体阻塞或需要决定时报告，不逐阶段播报 CI。

前端单元测试直接使用 Node test runner，不需要 Gateway 或浏览器，在 CI 中运行一次。测试分层与覆盖归属见 [测试策略](docs/test-strategy.md)；审查和重构测试时可使用 [Unit Test Review skill](.agents/skills/unit-test-review/SKILL.md)。

## WebUI 热重载

开发时可以由 Vite 提供 WebUI，Gateway 保持独立运行。Gateway 的 `listen` 指向后端端口，`public_url` 指向浏览器实际访问的 Vite 地址；Runner 的 Gateway 地址同样指向此公开地址。

例如 Gateway 监听 `127.0.0.1:8080`，`public_url` 为 `http://127.0.0.1:5173`：

```sh
cd web
npm ci
WAKEIT_DEV_GATEWAY=http://127.0.0.1:8080 \
WAKEIT_DEV_PUBLIC_URL=http://127.0.0.1:5173 \
  npm run dev -- --host 127.0.0.1 --port 5173 --strictPort
```

Vite 将 `/api` 和 `/health` 代理到 Gateway，包括会话、写操作和 SSE 实时日志；修改 React 或 CSS 后会自动更新浏览器。开发服务器可由用户服务管理器独立托管，避免依赖临时测试进程。生产部署仍使用嵌入 Gateway 的静态 WebUI。

## 集成测试

```sh
cargo build --locked --workspace --bins --examples
python3 scripts/integration.py --pg-bin /path/to/postgresql/bin --keep-running
```

不维护浏览器测试套件。界面改动使用合成数据进行人工验收；大规模样式迁移使用本地一次性脚本，对同一浏览器、字体、视口、语言和主题下的迁移前后页面截图做对比。脚本、基线、截图和差异图放在忽略目录 `.local/`，不提交，也不上传到 GitHub。具体流程见 [测试策略](docs/test-strategy.md)。

测试凭据和端口保存在忽略目录 `.local/`，不得提交。测试栈与日常预览独立，退出验收时关闭临时服务。

## WebUI 翻译

界面使用 i18next / react-i18next，资源位于 `web/src/locales/zh.json` 和 `en.json`。保留现有文案含义，只翻译界面文字，不翻译任务指令、报告、日志、路径、模型 ID 或第三方返回的内容。新增文案需同时维护两种语言；整句使用插值，数量使用复数资源，不拼接不同语序的句子。模块级目录使用延迟读取，组件订阅语言变化，避免切换时重建页面或丢失草稿。

首次使用按浏览器语言选择中文或英文，之后使用浏览器保存的 `wakeit-language`。顶栏版本号右侧可切换，登录页也提供同一入口。复制的 Agent Prompt 独立于界面语言。

翻译目录与插值由前端单元测试检查。界面观察范围随改动确定：修改翻译、主题或响应式布局时查看对应状态，不对每个小改动机械遍历全部语言、主题和宽度组合。截图使用合成账号、报告和设备，不为文案位置或控件尺寸增加长期维护的测试。

发布产物构建使用 `.github/workflows/release.yml` 的手动运行，默认只构建并上传 Actions artifacts。只有匹配包版本的正式标签才发布 Release。Gateway 标签使用 `gateway-v0.4.2`，CLI 使用 `cli-v0.1.1`。手动运行选择产品；CLI 不构建 WebUI，Skill 随 CLI 发布。安装脚本各自默认版本随对应产品发布更新；CLI 更新器只读取 `cli-v*` 正式发布。历史 `v*` 标签保持不变。发布前同步中英文 changelog，发布正文只提取目标版本条目。已推送标签不改写；后续发布通过 Issue、feature branch 和 PR 准备。

[README Crafter skill](.agents/skills/readme-crafter-skill/SKILL.md) 是维护 README 的仓库内技能；写完核对命令、配置、链接和真实发布状态。

main 的服务器保护要求通过 PR、分支保持最新、讨论解决，以及 `verify-result` GitHub Actions 检查（汇总实际修改范围选中的前端、CLI、Gateway、共享代码及服务集成结果）；禁止删除和强推，对管理员同样生效。不要求额外的外部审批人数，以支持单维护者通过合格 PR 合并。
