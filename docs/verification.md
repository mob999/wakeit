# 首版验证记录

验证时间：2026-10-14。环境为 macOS arm64、Rust 1.98.1、Node.js 22.23.2、PostgreSQL 16.15、NATS Server 2.15.0。

## 已通过

- cargo fmt --all --check。
- cargo clippy --all-targets -- -D warnings。
- 26 个 Rust 核心及 Trace 测试：配置、参数传递、时区、夏令时、去重恢复、UTF-8 日志、结果限制、进程树取消、超时与监控不确定性。
- 33 个真实 PostgreSQL/NATS 集成场景，使用两个独立 Gateway、WebSocket Runner 和同步/异步模拟 Agent。
- 45 个 Chromium 浏览器场景：时区表单及预览、任务创建/编辑/执行/结果、Runner 能力及注册、Viewer 权限、邀请、主题与 API Key、模型发现/失败/空列表、紧凑表单与自动 ID、全局搜索和分页、产品文案与状态样式一致性检查，以及设置页草稿保护、角色变更、私有 Token、分页和响应式布局；增加了 Agent 搜索定位与任务预填、设备分页及刷新保留、设备/Agent 筛选交集与能力隔离、四个工作台窄屏列表详情切换、面包屑层级及离开保护；新增 Agent 整宽表格、单行名称、官方 Logo 与图片失败首字母替代、无边框操作图标的浏览器验证。
- 上一轮 Codex、Pi、OpenCode 的纯文本适配真实试跑已通过；本轮 Codex、OpenCode 结构化输出真实试跑零退出码，工具执行与最终 Markdown 均正常。Pi 初次试跑遇到本机 Magpie 未运行导致的 Connection error；启动 Magpie 后，同一 provider/model 的短请求零退出码并返回正确结果。Pi 增量及工具路径另外由原生协议 fixtures 覆盖。
- Python HTTP 异步适配示例：models/submit 参数契约试跑通过；原 submit/status/cancel 试跑记录保留。
- 真实 Runner API 模型发现：Codex 返回 7 项、Pi 返回 285 项、OpenCode 返回 92 项；另验证了空列表只提供手填。目录、配置和账号会影响模型目录。
- 上一轮 CI 的 Windows 两个 Node 参数传递测试触发了 5 秒启动预算；本轮将这两项契约测试的预算调整为 30 秒，保留实际执行超时测试的原限制。统一工作台与官方 Logo 两轮提交的 Linux、macOS、Windows CI 均已全部通过；运行内容变更提交后再次运行工作流。
- Wakeit 品牌、版本 0.1.0、桌面筛选行等宽与垂直对齐、多范围筛选及 390px 窄屏弹窗通过浏览器核对。
- Compose 单机/HA 和 CI YAML 语法检查。

集成场景：

- shared authentication across Gateway instances
- cookie write CSRF protection
- member roles and single-use invitations
- settings timezone catalog, member pagination, last-admin protection and private API Key pagination and expiry
- Runner enrollment, WebSocket transport and local adapter detection
- workspace timezone defaults and structured input errors
- optimistic revisions, idempotent trigger and literal Unicode/stdin handling
- server-side Task and Run pagination, literal fuzzy search, filters and cross-Gateway reads
- native directory queries across Gateways, pagination and symlink boundaries
- models command contract, no interface, empty, failure, malformed output and timeout
- task parameter validation, adapter defaults and durable resolved snapshots
- duplicate delivery does not restart an executed Run
- already acknowledged messages can be cleaned without repeated broker errors
- NATS API and subject isolation for device credentials
- concurrent manual triggers and overlap protection
- two Gateway schedulers create exactly one occurrence
- async submit/status and confirmed cancellation
- sync cancellation terminates command process
- JSON output interrupted by cancellation keeps the confirmed cancellation state
- async recovery after SIGKILL without resubmitting
- uncertain submit is not retried and requires explicit resolution
- latest missed occurrence coalescing and pause revocation
- paused and superseded work is removed from JetStream
- skip misfire does not execute stale offline work
- event sequence deduplication and terminal-state protection
- broker restart preserves outbox and durable consumers
- Gateway crash preserves the running process and its durable result
- SSE and cursor reads across Gateway instances
- device revocation rejects subsequent API access and execution

## 待其他环境验收

Claude Code 模板已通过配置校验，本机未安装该 CLI，尚未进行真实试跑。Linux/Windows 的构建与核心进程测试已通过 CI；真实 Agent 试跑和模型发现只在本机 macOS 验证。Docker 未安装，因此 Compose 镜像启动、三节点 JetStream、HTTPS/WSS 入口与完整备份恢复流程尚未实测。配置和操作说明已提供。

新增四个运行内容场景：验证 Tasks/Runs 图标与留白布局、Markdown 表格及代码、危险链接与 HTML 过滤、JSON/空结果/截断原文、解析快照、默认页签、手动选择保持，以及明暗主题和窄屏布局。

按钮重构新增五个浏览器场景：透明图标、Tooltip 的键盘与指针入口、文字提交、菜单组合、精确复制 Task/Run ID、标题计数清理，以及单线分隔的拖拽、键盘调整、宽度保存和双击复位；一级导航的拖拽、折叠与恢复、顶栏分界对齐、版本及账号位置、手机覆盖层，以及执行历史名称在可用空间内完整显示。

## 本机预览

本机预览使用独立配置和数据；当前 PostgreSQL、NATS、Gateway、Runner 与 Vite 热重载 WebUI 已由 macOS launchd 独立托管。具体端口、路径和服务操作记录保留在本机，不提交到仓库。已在原地址验证 CSS 的 Vite WebSocket 更新、登录、API、SSE 事件流与健康检查，并保留现有数据。生产部署时通过 gateway bootstrap 自行设置管理员密码。

scripts/integration.py --keep-running 仅用于创建隔离的测试栈；其配置和进程记录位于被 Git 忽略的 .local/integration-stack.json。不要使用该脚本恢复已有服务。

## 样式核对

参考 ghdiff 提交 21902f4cdd153b749787c5bff7a6c01a4b9521c8。中性色、字体、顶栏、按钮和列表密度已按源码对齐，详见 [样式基准](design.md)。截图由真实应用的浏览器测试生成。

设置页新增六个浏览器场景：验证后台刷新不覆盖草稿、字段范围、离开保护、时区搜索、保存失败、全局搜索跳转、成员分页与停用状态、自身降权后的权限刷新、Token 重复提交与一次性密钥展示、复制/撤销失败，以及 390px/1440px/2200px 和明暗主题布局。

## Agent 过程与两步任务编辑

本轮新增 6 个 Rust 测试，覆盖四类原生协议、公开内容过滤、Pi 增量快照、分片 UTF-8/CRLF、损坏与超大记录、原生明确失败及重试恢复、OpenCode 多段报告、辅助命令进程树取消、实时 Journal 落盘、超过 5000 条日志后的独立结果、断线待传数据保留和共享容量。

服务场景增加原生辅助命令/退出码、第三方实时事件和 Markdown 结果、事件模式取消，以及 Trace 重投去重、实际发生时间、共享容量与过期标记。已有两个 Gateway 的调度、权限、异步恢复、取消和 SSE 场景继续通过。

浏览器增加 5 个场景：任务/时间两页校验、下一步无 PUT、模型和调度草稿保留、四种执行类型、失败/冲突/固定 ID 后缀、提交禁用、独立 Trace 游标补读、重复及迟到事件、并行步骤、准确耗时、Markdown 和缺失/等待/截断/过期状态。暂停任务编辑后仍保持暂停。已更新明暗和 390px 截图。

本轮没有在本机执行 Linux/Windows 真实 Agent CLI，交由现有 CI 矩阵验证编译与核心进程测试；Claude Code 本机未安装，结构化协议使用 fixtures。Pi 连接故障已定位到本机 Magpie 服务未启动；恢复服务后实际请求成功，未更改 Agent 配置或登录凭据。

## 统一选择控件与 Run 展示

本轮仅调整前端组件，不改 API、数据库或执行协议。隔离服务的 33 个集成场景通过。浏览器新增五个场景，覆盖共用选择器的搜索、键盘、焦点返回、离线和未检测选择、枚举、字段联动、明暗及窄屏；Run 成功强制展示结果一次、延迟结果、折叠与工具状态保留、失败与无 Trace 输出，以及日志断线重连、去重、跟随和执行信息保留。

Run 保留执行/详情两页，按过程→结果组织。折叠标题承载操作，正文与输入输出采用统一无框排版，消息和工具共用标题结构。更新选择器、过程、Markdown 结果和窄屏截图；前端构建、格式与类型检查通过。

## 全页面内容容器统一

采用 Runner Agents 图片基准：细边框、6px圆角、浅色表头、分隔行。新增两项浏览器场景，遍历 Tasks、Runs、Runner概览/Agents及三类Settings，验证1440px明暗、390px窄屏和2200px宽屏的边框、颜色、内容宽度和溢出；另验证登录、编辑和搜索弹窗的容器与控件。原有42个场景通过；33个隔离服务场景继续通过。最近执行与执行历史合并成一个表头，窄屏保留分区名。账号、任务、设备身份、模型和执行规则均未修改。

最近执行增加设备/Agent与执行时间列的实际位置校验，表头与数据行左边缘一致；相关7个场景在单表头和共享列宽调整后通过。

任务指令独立成第二行卡片，位于调度与执行内容之间；已验证卡片顺序及指令原文在各宽度和主题中保持一致。

## 品牌、技能、更新与系统服务

README 按固定提交的 README Crafter 重写，引用与安装命令核对通过；两项 skill 校验通过。Wakeit skill 独立复制后的引用、任务/适配 YAML 与事件示例通过 Schema 校验。同步事件示例以及 HTTP 异步包装的 models/submit/status/cancel 在隔离本机环境验证通过。

Rust 共 36 项测试、fmt、clippy、前端构建和 45 项最新热重载浏览器回归通过；33 项 PostgreSQL/NATS 集成场景通过。更新覆盖正式版本排序、认证/无发布区分、URL 限制、大小和 SHA-256、manifest 版本/平台/文件大小，隔离可执行副本真实替换及失败候选保留原文件。私有 GitHub 实际检查返回 no_release，未创建正式发布。发布打包的完整性/重复及缺失平台/标签校验通过，两个 Actions 工作流通过 actionlint。

Gateway 环境文件启动在隔离 PostgreSQL/NATS 上验证 ready，并验证显式文件值覆盖无效继承值。三平台系统服务文件通过结构检查；本机完成 plist、shell 语法检查。Linux systemd 与 Windows PowerShell 原生检查放入 CI；未在三平台重启机器，不宣称真实开机验收已完成。原有本机服务及账号/任务/设备/Agent 凭据未重新初始化。

侧栏展开显示 Logo、Wakeit 和 PanelLeft；折叠只保留点击可展开的 Logo。最近执行采用独立设备/Agent 列及共享网格表头，桌面宽松列距、窄屏两行布局和完整名称验证通过；明暗截图已从原热重载地址更新。

远端本轮 Linux/macOS 与服务浏览器集成已通过；Windows 暴露旧 Trace 测试的 250ms 固定启动等待竞态。改为先等待实际 Trace 持久化，随后通过文件信号放行测试 Agent；断言仍要求执行结束前收到事件，本机 Trace 回归通过。

Windows 同一大输出 fixture 另出现 5500 条 durable Journal 写入超过 30 秒测试预算，Windows 的该压力 fixture 单独改为 180 秒；正式任务/取消/超时语义与专门超时测试未修改。

五个原生目标（Linux x64/ARM64、macOS Intel/ARM64、Windows x64）均通过 Release 构建及可执行文件试跑；后续装配修正了目录和文件同名造成的匹配重复，回归 fixture 已采用真实 Actions 目录结构。Windows 文档/服务检查显式使用 UTF-8，避免默认 CP1252 读取中文；发布工作流保持手动只产出 artifacts，不创建 Release。

## API Key 设置（2026-10-15）

设置页统一称为 API Key，标题不再显示数字方框，创建入口使用透明 Plus 图标。新 Key 默认不过期，支持预设期限和自定义本地时间；接口接受可空 expires_at，拒绝过去时间。数据库结构及已有 Key 到期时间保持不变。

36 项 Rust 测试、33 项 PostgreSQL/NATS 集成、45 项 Chromium 场景及前端构建通过。新增验证覆盖默认/显式不过期、未来期限、无效及过去时间、到期和撤销后的跨实例鉴权、预设及自定义期限、创建错误保留输入、标题计数移除和 Plus 入口。原热重载地址的明暗主题及 390px 弹窗截图已更新；本机二进制已同步，Gateway ready 检查通过。

## Web 账号管理与任务归档（2026-10-15）

36 项 Rust 测试、35 项 PostgreSQL/NATS 集成及 49 项 Chromium 场景通过，fmt、clippy、前端构建通过。覆盖成员创建、独立角色和状态修改、凭据撤销、自身重置退出、最后管理员并发保护；归档撤销排队执行、运行继续、保留历史、只读和重试限制、分页、权限、暂停恢复及 CLI 操作。另以 24 个同时成员更新、24 个同时 Key 创建验证事务内鉴权，避免连接池等待。

旧库升级在隔离 PostgreSQL 中验证备份、原数据保留、单一迁移记录、重复执行和未知校验和拒绝。原本机服务已完成升级，逐表行指纹一致；账号、Key、任务、设备和历史未重建，Runner 未重启。CLI 和热重载预览仍为 0.1.0，Gateway ready 通过，明暗主题及窄屏截图已核对。


## Moments 结果时间线（2026-10-15）

37 项 Rust 测试、36 项 PostgreSQL/NATS 集成场景，以及 57 项 Chromium 场景（全站回归与修复项复测）通过；前端构建、fmt、clippy 通过。Moments 为默认主页，Tasks 路由、全局搜索及归档任务链接使用 /tasks。界面不按日期或条目数量自动折叠，统一预览与阅读全文。

验证包括完成时间与跨夜分组、工作区时区和重复夏令时、同时间 ID 游标、两层分页、全量计数、2048 Unicode 字符预览、JSON 与原始截断标记、归档历史及跨 Gateway Viewer 读取；无新增数据库迁移。浏览器覆盖三种角色、空列表、加载与全文失败重试、复制全文、无自动刷新、全文展开及返回恢复、刷新新增结果后的阅读锚点，以及明暗主题 390/1440/2200px。

本机二进制与原热重载地址已更新，实际历史结果读取和 Tasks 跳转验证通过；Gateway ready 返回正常。未重建数据、账号、设备身份或 Agent 凭据，Runner 未重启。源码与截图中的报告使用测试 fixture，真实报告截图仅留在忽略目录。

Moments 后续视觉修正将日期对齐至卡片左侧，并以单一连续轴线贯穿日期；设备与 Agent 移至表头，窄屏在同一表头内换行。8 项专项回归、构建及明暗主题 390/1440/2200px 截图通过，额外断言轴线覆盖跨日距离、日期对齐、正文无设备信息及窄屏无溢出。


## 工作目录选择（2026-10-15）

63 项 Chromium 全站回归、6 项目录专项复测、前端构建和文档检查通过。独立 PostgreSQL/NATS 测试栈的 36 项集成场景仍通过；没有改 API、协议、数据库或任务执行语义。

目录选择改为路径输入与紧凑列表，默认进入已选目录或唯一根目录。覆盖选择后重开、单击选择、双击及箭头进入、方向键、Tab、Enter 与 Ctrl/⌘ Enter、返回与取消、中文空格、Windows 原生路径、200 项后的筛选、隐藏目录、目录删除、越界错误、校验期间关闭及重复提交保护。明暗主题 390/1440/2200px 截图已更新。

本机 mac Runner 配置备份后将 allowed_roots 调整为用户目录，确认无活动执行后重启并等待设备租约交接；原设备身份及 Agent 凭据保持不变。原热重载地址通过真实目录选择、Documents 重开与 /tmp 越界拒绝检查，未保存测试任务。


## 原生会话与搜索间距

- 会话专项覆盖精确ID/目录/持久化检查、模板变量、跨进程互斥、未知状态占用、交互终端交接与后代清理；Session元数据在日志预算耗尽、重启和7天清理后仍可读取。
- PostgreSQL/NATS两Gateway隔离集成覆盖新建与绑定重试、跨Task会话锁、缺失/错误原生ID进入unknown、断线回传、两层内容上限和本地离线续聊；归档、取消、调度和SSE原回归保留。
- 全部72项浏览器测试通过，含9项会话/搜索专项；65项Rust测试和42组完整服务集成通过。截图：[任务1440浅色](screenshots/session-task-1440-light.png)、[任务1920深色](screenshots/session-task-1920-dark.png)、[任务390深色](screenshots/session-task-390-dark.png)、[续聊1440浅色](screenshots/session-resume-1440-light.png)、[续聊1920深色](screenshots/session-resume-1920-dark.png)、[续聊390深色](screenshots/session-resume-390-dark.png)。搜索首卡距分割线12px，最后分组无多余外边距。
- 本机Codex、Pi、OpenCode已用隔离目录完成有限真实新建、持久化检查、绑定续跑及同ID复核。Claude Code未安装，使用协议及本地元数据fixtures验证，不声称真实调用或TUI试跑通过。
- pre-archive与pre-session旧基线分别验证事务升级、私有完整备份、数据保持、幂等和未知基线拒绝；全新数据库仅运行唯一初始迁移。

- 已有会话目录覆盖Admin/Operator权限、双Gateway、目录边界、中文搜索、20条游标分页、无接口和空列表；三种已装Agent的只读真实查询已通过。原生命令导出覆盖POSIX实际执行、中文/空格/引号、注入字面量与目录切换失败，以及PowerShell引用/路径；不导出prompt或环境值。
- 会话选择截图：[1440浅色](screenshots/session-list-1440-light.png)、[390深色](screenshots/session-list-390-dark.png)。
