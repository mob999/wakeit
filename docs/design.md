# WebUI 样式基准

视觉基准为 [ghdiff](https://github.com/GalvinGao/ghdiff)，主要对照 src/globals.css、ReviewHeader、ReviewSidebar、PullRow 与 ui/buttonClass。wakeit 的组件独立实现，使用同一组中性色和控件尺度。

| 项目                 | Light    | Dark     |
| -------------------- | -------- | -------- |
| 主画布               | #ffffff  | #0a0a0a  |
| 顶栏、一级导航底色 | #f5f5f5  | #171717  |
| 第二侧栏底色 | #fafafa | #121212 |
| 选中行、菜单         | #ffffff  | #1f1f1f  |
| 细边框               | #e5e5e5  | #262626  |
| 正文                 | #0a0a0a  | #fafafa  |
| 次要文字             | #525252  | #a3a3a3  |
| 主按钮               | 黑底白字 | 白底黑字 |

使用系统无衬线字体；命令、配置、Cron、路径、日志使用平台等宽字体。顶栏 44px，小按钮 28px，圆角 6px，图标保持单色。状态使用与 ghdiff 一致的 GitHub Primer 成功、等待、失败颜色。

布局保持可调整宽度的列表与详情工作台。列表使用紧凑行和轻选中背景；详情使用分隔线、配置和日志区域。后续页面沿用这些变量与组件，避免另起一套颜色、字体、间距体系。

滚动条沿用 ghdiff 的原生 CSS 样式：8px 透明轨道，滑块两侧各 2px 透明留白、圆角和次要文字色，悬停及拖动逐步加深；Firefox 使用标准 thin 滚动条，高对比度模式保留浏览器默认。列表、详情、弹窗、编辑框和横向滚动统一使用该样式。

文本输入、文本区和下拉控件聚焦时只加深原有 1px 边框，不叠加外圈或阴影；文本区不提供拖拽缩放手柄。

## 0.1.0 工作台更新

顶栏左区展示 Logo、Wakeit 与折叠按钮；中间展示当前页面面包屑及透明图标操作，右区展示弱化等宽 Gateway 版本号、主题按钮和用户菜单。删除工作区圆点/文字、顶栏标语和侧栏重复版本。侧栏图标为 ListTodo、History、Monitor、Settings2（搜索范围 Agents 使用 Bot），保持单色和 16px 尺寸。

目录选择器使用同一套 Radix 弹窗与紧凑行。先选 Runner 和目录再查询模型，适配能力决定是否提供搜索下拉，失败、空列表和无接口时采用手填，不显示静态或旧结果。模型、目录、任务编辑器和布局独立为组件。

## 紧凑任务编辑与全局搜索

任务编辑统一为“任务 / 时间”两页。任务页显示名称、指令、Runner/Agent、目录和模型；其余 Agent 参数折叠在高级设置中。任务与时间页可自由切换；下一步不校验、不保存，保存时统一校验。时间页直接显示调度类型、时区、输入和五次预览，超时及执行策略折叠；手动执行隐藏时间输入和预览，在该页保存。任务 ID 在内部由名称（中文转拼音，英文保留大小写）加随机后缀生成，编辑时保持 ID；表单不展示 ID 或生成过程。

主侧栏提供唯一搜索入口，可通过 Cmd/Ctrl+K 打开。搜索弹窗沿用工作台的中性色、细边框、紧凑控件和单色图标。范围可多选 Tasks、Runs、Runners、Agents、Settings；不选则跨范围搜索。选中范围后显示对应筛选项，结果按对象分组；点击结果进入详情，应用筛选后各页面共享筛选状态，清除筛选恢复全部列表。Tasks、Runs 及任务执行历史均使用服务端分页，每页 20 条。第二侧栏只保留列表、操作和页码。

产品文案仅包含字段、数据、操作与必要的错误/后果说明。删除页面简介、底部自动更新说明、并发编辑实现说明、接入协议说明及登录页标语；开发约定与适配教程保留在文档中。

显示品牌统一为 Wakeit，命令及配置标识保留 wakeit。搜索筛选行使用固定范围列与等宽控件列，重置通用表单的底部外边距；范围名称与控件纵向居中对齐，窄屏将范围名称单独放在行首。

## 统一工作台

Tasks、Runs、Runners、Settings 共用可调宽度的列表与详情骨架；第二侧栏背景、选中行、详情标题、分隔线及滚动一致。第一侧栏保持唯一搜索入口，不提供独立 Agents 导航。窄屏显示列表，选择后进入详情，通过面包屑返回列表，不产生横向滚动；深链接直接进入详情。

Runners 每页 20 台，列表显示名称、连接状态、系统和并发占用，按名称及 ID 排序。选中对象通过 URL 保留，查询刷新不改变选择。详情有概览、Agents 两个页签；概览展示系统、版本、并发、最近连接和执行范围。创建任务预选 Runner，吊销位于 Admin 的设备操作菜单。

Agent 完整配置仍属于 Runner。Agents 页签展示该设备实际上报的名称、版本、检测状态；展开一行查看执行模式、取消能力、模型发现能力及只读参数默认值。设备连接状态与 Agent 检测状态分别呈现。创建任务预选 Runner 与 Agent；模型目录仍在任务编辑器选定 cwd 后查询。适配模板和 Schema 通过 CLI、API 与文档提供，设备页不展示模板入口。

全局搜索保留 Agents 范围，结果按 Runner + Agent 标识，点击进入 `/runners?runner=ID&section=agents&agent=AGENT_ID` 并展开该行；点击具体结果清除另一个设备/Agent 范围中的冲突条件以保证精确定位。应用 Agents 筛选进入设备的 Agents 页签；设备须有匹配的 Agent，详情只展示匹配项，与 Runners 筛选取交集。同名适配的能力不跨设备合并。

Settings 第二侧栏展示工作区、成员、API Key 及归档任务；分类标题合并至顶栏面包屑，卡片占满可用宽度，滚动位于详情区域最右侧。设置正文、字段与控件统一 12px，列头及次要信息 11px，使用同一系统字体；时间、路径与配置保留等宽字体。成员、API Key、归档任务和实例账号的表头与数据行共用列轨道、16px 横向间距及 28px 操作列。工作区名称、时区和保留天数共用值列，不显示多余的“值”列头；创建放表头 Plus，重命名与退出放工作区名称行。设置行最小 48px、按钮高 28px，窄屏字段上下排列。

工作区时区可搜索选择，保留天数输入宽 96px，单位在控件外；修改后出现取消/保存栏。成员角色为文字菜单；邀请及 API Key 创建进入弹窗，密钥仅显示一次。成员与 API Key 每页 20 条，总数仅在分页区域显示，详情标题不显示计数。API Key 创建使用 Plus 图标，默认不过期，可选择预设期限或自定义时间。

设置对所有角色常驻，默认进入工作区。工作区配置所有角色可读、仅管理员可修改；成员管理仅管理员可见，归档任务仅管理员和读写可见，个人 API Key 对所有普通工作区成员开放。演示工作区只展示只读工作区设置和创建工作区入口。`/settings?section=workspace|members|tokens` 和全局搜索定位对应分类，手动选择清除 Settings 范围的筛选。未保存保护覆盖分类、侧栏、全局搜索、面包屑、退出登录和页面关闭；后台刷新不覆盖输入。

详情层级统一使用面包屑：Tasks → 任务、Runs → 执行、Runners → 设备 → Agents → 当前 Agent、Settings → 分类。祖先节点可点击，当前节点只显示文字；窄屏同样通过根节点返回列表，不提供单独的返回按钮。设置面包屑遵守未保存保护。

Runner 的 Agent 表格占满详情内容宽度，采用细边框、6px 圆角、弱化表头和统一 48px 行高。列为 Agent、版本、检测状态、操作；行内创建任务使用无边框 Plus 图标和可访问名称。Agent 只显示一行名称，旁边使用配置中的官方 Logo；缺省或加载失败显示首字母。

## Tasks / Runs 列表与运行内容

两个列表使用相同的三行结构：任务名称与状态、Runner 图标和名称及 Agent Logo 与显示名、时间。字段之间使用图标和留白，不使用装饰分隔点；Run ID 与触发来源仅出现在详情。Runs 面包屑为 Runs → 任务名称 → 执行时间，任务节点可进入对应任务。

运行名称、执行时间与操作合并至顶栏面包屑及操作区，正文不重复显示大标题；状态仅在左侧列表展示。页签为执行、详情。执行页固定过程→结果；运行中展开过程，首次结果就绪时，仅在跟随最新时折叠过程、展开结果，每个 Run 一次；阅读历史时保留过程并显示“结果已就绪”入口，用户点击后再定位结果，之后手动展开不被刷新覆盖。成功无结果、失败、取消及未知状态展示必要提示，不将日志当报告。详情页日志默认展开、执行信息默认折叠；设备、目录、解析参数、时间、来源、完整 ID、任务 YAML 和状态事件均保留。切换页签不丢失工具展开、原文模式或跟随输出状态。

字符串结果使用 Markdown 阅读视图，支持表格、列表、代码块及链接，并可切换原文或复制。JSON 结果保留结构化原文，不猜测摘要；成功空结果显示未返回结果，持久化截断结果明确标记。阅读视图沿用中性色、紧凑字号与细边框，HTML 经白名单清理后展示，仅 span 可带受限文本样式；危险链接协议不进入可点击链接。

## 按钮与工作台边缘

工具栏和行内操作统一使用 28×28px 透明图标按钮，图标 16px，圆角 6px，无可见边框、底色和阴影。默认次要颜色，悬停时出现轻灰背景；取消执行、撤销 API Key 的图标悬停时使用失败色。图标始终可见，通过可访问名称和 Radix Tooltip 标识操作；悬停延迟 400ms，键盘聚焦立即显示。

新建、创建、连接设备和邀请成员统一使用 Plus，不使用带设备、成员或列表的组合加号。重新执行 Run 使用 Redo2（向右弯箭头），刷新数据、读取重试和日志重连使用 RefreshCw（双向循环箭头），区分是否会启动新的执行。按钮的名称、权限、确认流程及提交状态保持原有行为。

公共 Button 使用 ghost、solid 两种外观；弹窗和表单的提交保留文字主按钮，次要操作使用透明文字按钮。目录、模型等输入控件保留边框，刷新等辅助操作采用图标。导航、页签、搜索范围和角色菜单保留文字。

移除 Tasks、Runs、Runners 侧栏标题计数、Runs 装饰波形、执行历史及 Agents 页签计数。Task、Run 顶栏操作区分别提供复制真实 Task ID、Run ID 的图标，复制成功仅短暂显示 Check；Task 标题不再重复打印 ID。分页总数继续放在列表底部。

分隔线参考 [ghdiff PaneResizeHandle](https://github.com/GalvinGao/ghdiff/blob/21902f4cdd153b749787c5bff7a6c01a4b9521c8/src/components/PaneResizeHandle.tsx)：列表自身的 1px 边框是可见分界，8px 的透明拖拽区域覆盖其边缘，不占额外布局列。悬停、聚焦、拖拽时出现细高亮；支持方向键调整、双击恢复默认宽度，保留用户宽度设置。

一级导航同样可拖拽，默认宽 144px，可调 128–280px；折叠后宽 44px，仅显示导航图标及 Tooltip。保存展开宽度与折叠偏好，展开恢复此前宽度；拖拽即时响应，折叠使用 200ms 宽度过渡并遵循减少动画偏好。两处分隔线都使用同一组件；内容区自动预留宽度，窗口变窄不覆盖已保存的宽度。

顶栏左区与一级导航共用宽度和 1px 分界线，分隔线贯穿顶部。展开时显示 Logo、Wakeit 与折叠按钮；折叠后只保留可展开的 Logo。版本和用户菜单一直位于右上角。手机默认图标导航，展开以覆盖层显示文字导航，关闭后恢复图标，不覆盖桌面偏好。

执行历史和最近执行行按内容分配 Runner、Agent 名称的宽度，不再固定为 80px。空间足够时显示完整名称，窄屏分成两行并保留时间和状态。

Agent 过程采用细分隔线的纵向列表，显示类型图标、标题、状态和已知耗时；工具/命令展开输入、输出，消息使用与结果一致的 Markdown 渲染。只呈现 Agent 的公开事件，不增加平台调度链路或推理内容。能力缺失、历史未记录、等待、截断和过期分别显示简短状态。

## 选择控件与 Run 内容层级

任务编辑器的 Runner、Agent、模型和枚举共用 ChoicePicker。Runner 和 Agent 支持搜索，状态单独右对齐；Agent 展示等字号 Logo 与名称。弹层与触发器等宽，支持键盘、焦点恢复和窄屏边界；不改变离线选择、模型发现和手填规则。

全部工作台沿用 Runner Agents 表格的容器：1px 细边框、6px 圆角、无阴影，浅色表头约44px、数据行48px、卡片间距16px。内容区左右留白18px，窄屏Run区12px，卡片占满可用宽度。属性以标签/值行展示，内部使用分隔线，不给字段和Trace节点重复套卡片。

任务概览依次展示调度、指令、执行内容、最近执行；指令独立成卡片，位于第二行；最近执行与执行历史只保留一行表头，名称和列名同排，窄屏保留名称。表头与数据行通过共享列轨道对齐，设备/Agent和执行时间不再按每行内容独立计算宽度。Run的过程/输出、结果、日志和执行信息采用同一折叠卡片，操作放表头右侧。Trace使用动作、状态、耗时的固定对齐列，真实未知耗时以破折号表示，不推断时间。原文、输入输出和配置留在展开内容内；工具参数和结果正文的代码不加底色，结构化 JSON 与声明语言的代码块保留语法高亮。

Runner概览、工作区设置、成员/Token表格和搜索结果分组共用相同边框、表头与行样式。登录与全部弹窗统一6px边框及标题/内容/操作区域；任务编辑高级分组、目录和选择器沿用相同控件语言。报告与过程共用页面滚动，长日志保留跟随与独立滚动。

Settings 新增归档任务分类，Admin、Operator 可见；保持原默认分类。成员表格为成员/角色/状态/操作四列，标题提供 Plus 新建账号与邀请图标，行内使用角色菜单及 MoreHorizontal 操作菜单。归档表格展示任务、Runner、Agent、归档时间、恢复图标；均使用现有容器和分页。任务详情提供 Archive 图标及确认；已归档任务只读，恢复后保持暂停。

### Moments

Moments 位于 Tasks 上方，是 `/` 默认主页；Tasks 使用 `/tasks`。不使用第二侧栏。沿用整宽细边框容器、弱化表头、透明图标按钮和等字号 Agent Logo。完成时间按工作区时区分组，左侧时间与贯穿所有日期的中性竖线节点，日期与右侧卡片左对齐；卡片表头包含任务名称、设备、Agent、状态与操作，正文仅展示结果及必要原因。窄屏把时间移至卡片上方，避免挤压正文。

日期不折叠，不按单条数量提供例外。所有结果采用预览与阅读全文，预览最高 240px；短结果不显示展开入口。每批 14 个日期，日期内每批 20 条。首次进入加载，之后仅用户刷新，不轮询、不因聚焦或重连自动更新。失败保留实际原因，报告支持 Markdown、表格、代码与 JSON。返回运行详情后保留当前访问的阅读位置，浏览器重载重置；正文不持久化到本地存储。

### 目录选择

参考 [VS Code Simple File Dialog](https://code.visualstudio.com/docs/editing/getting-started/tips-and-tricks#simple-file-dialog)，使用路径输入与紧凑目录列表。打开时进入已选目录；未选择时进入本次访问最后选择的目录，首次使用只有一个允许根目录则直接进入。多根目录可从根目录按钮切换，不扫描整棵文件树。

单击选择、双击或右侧箭头进入，方向键选择、Tab 补全进入、Enter 进入、Ctrl/⌘ Enter 确认、Alt+↑ 返回上级、Escape 取消。可以粘贴绝对路径，或输入当前目录内的名称筛选；筛选会继续读取该目录的剩余分页，不漏掉第 200 项之后的匹配。默认隐藏点开头的目录，提供显隐按钮。中文、空格和 Windows 路径按 Runner 原样保留。

确认前重新向 Runner 校验目录，删除或符号链接越界时不提交旧结果；等待校验时关闭弹窗不会迟到地修改任务。正文不增加实现说明。

## 顶栏上下文与正文起点

页面面包屑是唯一的页面标题，固定在 44px 顶栏内。Tasks、Runs、Runners、Settings 正文删除重复的面包屑、大标题与操作行；第二侧栏仍保留列表标题和新建入口。正文直接从页签或内容开始，卡片、表格及分区标题保留。

页面操作在面包屑右侧，使用细分隔线与版本、主题、账号区分开。窄屏将页面操作收进“页面操作”图标入口，保留原操作的权限、禁用和确认状态；仅查看列表时顶栏只显示列表名称。设置分类切换及根面包屑继续遵守未保存保护。

Moments 正文不再重复显示标题或刷新栏，桌面距离顶栏 16px，窄屏 12px。日期文字中心对齐时间线，不显示日期圆点；日期文字覆盖线条，条目节点和连续时间线保留。窄屏为日期预留左侧空间，长日期不超出边界。

## Markdown 扩展

结果、Moments 与 Trace 消息统一使用 react-markdown，启用 GFM、remark-math / KaTeX、rehype-highlight 和受限 HTML。带语言的代码块提供语法高亮；diff 的新增、删除分别使用成功与失败色，未知语言仍显示原文。行内公式使用 `$…$`，独立公式使用 `$$` 包围的块。

HTML 先经 rehype-raw 解析，受限文本样式处理后经 rehype-sanitize 清理，再生成公式与高亮。span 允许 color、background-color、font-weight、font-style、text-decoration；颜色允许 CSS 颜色名称、十六进制和 rgb/hsl 值，不接受 URL、变量、布局或任意 CSS。代码块里的 HTML 始终是源码。

mermaid 代码块按需加载 Mermaid，随明暗主题重绘。图表使用 strict 模式，清理后的 SVG 在独立无脚本沙箱内展示；不能覆盖工作台样式或启动页面回调。源码保留在紧凑折叠区；语法或加载失败显示简短状态并保留源码，不影响其余报告。正文和源码不进入本地存储。

参考：[rehype-sanitize](https://github.com/rehypejs/rehype-sanitize)、[remark-math](https://github.com/remarkjs/remark-math)、[Mermaid](https://mermaid.js.org/config/usage.html)。

## Workspace 导航

工作区使用稳定 `/w/{workspace-id}/…` 地址。工作区切换放在右上角用户菜单内，菜单列出可访问工作区，名称前不显示图标，当前项右侧显示角色和勾；顶部仅显示用户名，底部保留账号操作。创建、退出工作区位于设置的工作区分类，保留未保存保护与退出确认。权限展示名称为管理员、读写、只读（英文 Admin、Read/write、Read-only），接口角色值保持 admin、operator、viewer。一级侧栏只保留业务导航。切换先通过现有未保存保护，再加载目标工作区的新文档；每个文档拥有独立的查询缓存、搜索状态和 SSE，两个标签页不共享“当前工作区”。

工作区成员页只管理成员关系及邀请。全局本地账号的创建、启停与密码管理放在实例管理员专用页面，从账号菜单进入；不授予普通工作区 Admin 全局账号权限。演示区隐藏无效的设置及设备接入操作，服务端仍执行只读限制。

用户在顶栏、工作区成员及实例账号列表中直接显示用户名，不生成头像或首字母方框。Agent Logo 及缺省首字母标识保持原规则。

操作、请求和表单校验错误统一使用右上角 Toast，弹窗保持原有草稿与恢复操作；重复的同一请求错误不随轮询反复提示。实际运行失败原因、内容截断及 Agent 输出属于数据，继续留在相应内容区域。

## DAG 编辑与运行工作台

DAG 编辑器使用占满剩余高度的可交互画布，取消第二侧栏和图下编辑区。任务名称放在顶栏面包屑末级，直接编辑且两页可用；图表切换、Plus、搜索、撤销/重做、整理和输出设置集中在任务页工具区，底部操作固定。画布右上角只放缩小、放大、全屏，配置面板打开后避让；全屏展开编辑工作区并保留顶栏，退出不改变草稿与坐标。Agent 面板默认 200px，按 Runner 分组，可搜索和折叠；拖入和点击添加均绑定明确的 Runner/Agent。节点与端口保持中性细边框，不使用装饰网格或流动连线。

图节点采用同一白底上的紧凑三行，编辑、概览和运行统一 240×96，并共用布局尺寸。26px 设备/Agent 头部不加灰底或分隔线；42px 名称区顶部留 6px，名称整宽、12px 字号、18px 行高，固定容纳两行；26px 底栏显示身份和状态，三行共用 12px 水平内边距。设备长名和 Agent 长名通过 Tooltip 显示完整内容，菜单固定在编辑节点头部末端。输出节点在左下显示 14px 实心旗帜与 11px“输出 / Output”蓝色静态标记，无描边、底色或额外内边距；浅色 #0969da、暗色 #58a6ff。运行状态位于右下，保留原状态色；输出和状态在同一行垂直居中，输出身份不改变整卡底色和边框。表格复用输出标记并保留 Runner/Agent 列。输出设置、输出选择与输出节点文案保持中英文一致，配置字段和上游结果引用语法不变。

节点配置在点击后浮于画布右上方，距边缘 16px，初始宽 520px，可调宽并至少保留 320px 可见画布。标题固定、内容内部滚动，不改变画布尺寸；窄于 960px 时改为整宽覆盖。输出设置复用同一面板，指令可在工作台内展开。表格模式保持节点、前置、Runner、Agent、操作的共享列轨道，点击后仍使用悬浮面板，不做行内展开。字段使用白底标签/控件行、细分隔线；正文及控件 12px，列头和次要信息 11px，6px 圆角，透明操作图标。

首次无坐标图和主动整理使用 ELK，其他编辑保留节点位置与视口。移动、连线、改接、输出选择、删除和整理可撤销；文本使用原生撤销。共享坐标与 Task 一同手动保存，独立于 YAML 和执行快照。时间页保留整宽时间、执行策略卡片；切页和下一步不校验、不写入，保存时统一校验，错误使用 Toast 和字段/节点定位。

任务概览、运行图与编辑器复用同一套右上角缩小、放大、全屏控件；只读图全屏时保留选中节点和缩放，切回表格或 Escape 退出。DAG 运行执行页保留过程→结果两张卡片。过程内部图表不再叠加灰色节点表头，选中节点的名称、状态、耗时和操作合并为白底上下文行。结果按输出节点以白底分隔行展示，不套子卡片。详情只有一张节点详情卡片，唯一表头放节点选择、状态、续聊及核对；单节点不显示选择器。日志和执行信息采用卡片内部的白底折叠行，日志操作在折叠行右侧；执行属性沿用标签/值表格，不重复节点标题。

## Session continuation terminal

Run, DAG-node and Moments continuation entries open the same terminal modal. Use a compact 44px header and terminal body, without a property table, command card, footer or extra explanation. The header contains concise device/Agent context, an optional shared DAG-node selector, connection state, native-command copy and close. Keep 1px borders, 6px corners, transparent icon actions, 12px controls and 11px metadata. Desktop bounds are 1100px and 85dvh; narrow screens retain 8px margins and never create page-level horizontal scrolling.

Show device and complete native command inside the terminal before connecting; do not submit this text as input. Header copy always retains that command after native clear-screen operations. xterm owns Escape, Tab, arrows and Ctrl+C; Escape/backdrop never closes the modal. Ctrl+Shift+F6 moves focus between the terminal and header controls; closing returns focus to the source entry. All Wakeit errors use the top-right Toast, while Agent output remains terminal content. Explicit close awaits process cleanup; unexpected disconnect disables input while reconnecting for up to 30 seconds. No screen or input is persisted in browser storage.

## Result file previews

Admin and Read/write members can click explicit local links in a saved Run/DAG report or Moments to preview a current Runner file. Viewer can copy paths; demo workspaces never query real files. No separate preview_roots is configured: the exact saved link and current Runner allowed_roots bound access. Relative targets use the execution snapshot's task directory, which may differ from an adapter's overridden process directory; adapters should return absolute links in that case. Current files are not archived historical artifacts.

`POST /runs/{execution-id}/files/preview` accepts `{href}`. DAG uses the actual child execution ID. Gateway parses the persisted Markdown/HTML anchors and reauthorizes before delivery; arbitrary, code-block and nested preview links do not grant access. Runner uses the existing version-1 query channel with `read_result_file`, no shell/listener or database fields. At most 512 KiB raw content (further limited by NATS) is returned once: UTF-8 text/code, Markdown/JSON or complete PNG/JPEG/WebP. HTML/SVG stay escaped source; PDF, downloads, directories and special files are unsupported. Local image references do not automatically load. Preview body is memory-only and no-store; closing cancels delivery. Existing Runner identities and allowed_roots remain unchanged.

DAG 运行的节点上下文行使用 12px 名称、11px 状态和耗时，耗时保持弱化等宽字体；窄屏将名称与状态信息分为两行。节点菜单独立于触发器宽度，桌面宽 420px，窄屏保留 12px 页边距；名称与小字 ID 占同一身份列，状态和勾号各自对齐，仅纵向滚动。长名称单行省略并保留完整名称提示，支持按名称和完整 ID 搜索。

## 运行阅读与节点上下文

父 Run 状态与异常节点入口固定在“执行 / 详情”页签行右侧，不随页签、结果或选中节点移动。状态悬停或聚焦显示完整运行原因，过程和结果正文不增加独立诊断行。异常节点菜单使用对齐的名称／状态列，只统计当前失败、超时、中断或待核对的节点；跳过节点单独分组，自动重试等待和恢复后的旧失败不计入。选择后打开过程并定位到目标节点的当前执行，保留结构视图与各执行阅读状态。取消不虚构节点异常，没有匹配节点时隐藏入口；键盘关闭菜单返回触发器。

DAG 过程使用上下分区。图/表切换、搜索和结构折叠位于过程表头，节点表最大高度与图高为 min(280px,35dvh)，窄屏上限 220px；表头固定、结构独立滚动。收起结构不隐藏消息。选中节点上下文在阅读区吸顶，名称、实际尝试、状态、原因、耗时和已有操作对齐；点击节点只在上下文不可见时定位，不因状态刷新跳动。状态、耗时和操作靠右对齐；失败状态支持悬停或键盘聚焦，通过右对齐 Tooltip 查看完整原因，不在上下文栏平铺原因或增加展开行；历史原因、耗时、结果和续聊来自同一实际 execution。操作错误使用右上 Toast，持久原因留在内容中。

Trace 内部行最小 36px，正文 12px、状态和耗时 11px，采用细分隔线。历史消息默认摘要，最新非空消息默认展开；空消息不生成正文。工具输入/输出分别折叠，标签和代码统一 12px、20px 行高，以 8px 间距分组；代码与标签文字对齐，只保留 Trace 条目最外层分隔线。长内容先预览 12 行，完整内容可展开和复制。保留真实 ID、父子关系与事件顺序，不从日志补造 Trace。阅读页负责主纵向滚动，代码区只承担自身横向滚动。

阅读状态仅保存在内存并按工作区和实际执行隔离。手动展开优先于默认规则；全部收起停止自动展开，回到最新恢复跟随。距消息末尾 48px 内为跟随；向上阅读后不自动滚动，新事件提示更新数量，不收起正在阅读的消息。页签及尝试切换保留折叠、全文、锚点和跟随状态，迟到请求或快照不得覆盖另一执行。

## 第二侧栏层级与密度

Tasks、Runs、Runners、Settings 共用独立的第二侧栏语义底色与细边界。工具头高 44px，名称和分类 12px，元信息、状态、时间及分页 11px，不随窗口改变字号。Tasks/Runs 三行对象最小 64px，Runners 两行 48px，Settings 分类 36px；名称可省略，状态和操作不压缩。默认、悬停、选中与键盘焦点分别可辨，选中边框不改变尺寸，当前项具有 aria-current。列表和详情独立滚动，工具头和分页固定，滚动条保留稳定空间。

对象列表保持 350px 默认、270–560px 范围和原有宽度偏好；Settings 默认 240px、200–320px 范围并独立保存。拖拽、键盘调整和双击恢复沿用同一控件，窄屏继续通过列表/详情和面包屑切换。
