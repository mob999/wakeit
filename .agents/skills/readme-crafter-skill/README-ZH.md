[English](./README.md) | **中文**

<div align="center">

# README Crafter

**一个 Agent Skill，为每个项目量身定制诚实、结构清晰的 README——基于项目类型、受众和分发模式。**

[![License](https://img.shields.io/badge/License-MIT-111827?style=flat-square)](./LICENSE)
[![Agent Skills](https://img.shields.io/badge/Agent_Skills-Compatible-4f46e5?style=flat-square)](https://agentskills.io)
[![Python 3.10+](https://img.shields.io/badge/Scanner-Python_3.10+-3776AB?style=flat-square&logo=python&logoColor=white)](#scanner)
[![Portable](https://img.shields.io/badge/Portable-Shell_%2B_Markdown-22c55e?style=flat-square)](#agent-portability)

</div>

---

大多数 README 生成器的做法是扫描仓库、填充模板。结果读起来像一张表单，而不是项目的门面。

README Crafter 采用了不同的思路：它沿四个独立维度对项目进行分类——项目类型、分发姿态、目标受众、呈现风格——然后针对这个特定组合生成匹配的 README。它拒绝编造安装命令、伪造代码示例，或宣称源码中不存在的功能。

## 安装

### 一行命令（推荐）

自动检测你已安装的 Agent（Claude Code、Cursor、Codex、Gemini CLI 等 40+ 种），一键安装到所有 Agent：

```bash
npx skills add linhai0872/readme-crafter-skill
```

加 `--global` 安装到用户级目录，省略则安装到项目级。加 `--all` 跳过确认提示。

### 让你的 Agent 自动安装

复制下面的 prompt，粘贴给任意 AI 编程 Agent——它会自动完成安装：

```text
帮我安装 readme-crafter-skill：克隆 https://github.com/linhai0872/readme-crafter-skill.git
到我的 skills 目录并创建符号链接，让当前 Agent 可以使用。安装完成后验证 SKILL.md 是否可访问。
```

### 手动安装

<details>
<summary>克隆 + 符号链接</summary>

```bash
git clone https://github.com/linhai0872/readme-crafter-skill.git
```

然后创建符号链接到你的 Agent skill 目录：

```bash
# Claude Code
mkdir -p ~/.claude/skills
ln -s /path/to/readme-crafter-skill ~/.claude/skills/readme-crafter-skill

# 跨 Agent 通用标准
mkdir -p ~/.agents/skills
ln -s /path/to/readme-crafter-skill ~/.agents/skills/readme-crafter-skill
```

| 平台 | 用户级目录 | 工作区目录 |
|---|---|---|
| Claude Code | `~/.claude/skills` | `.claude/skills` |
| Codex | `~/.agents/skills` | `.agents/skills` |
| Cursor | `~/.agents/skills` or `~/.cursor/skills` | `.agents/skills` or `.cursor/skills` |
| Gemini CLI | `~/.agents/skills` or `~/.gemini/skills` | `.agents/skills` or `.gemini/skills` |
| OpenCode | `~/.agents/skills` or `~/.config/opencode/skills` | `.agents/skills` or `.opencode/skills` |
| Goose | `~/.config/goose/skills` | `.goose/skills` |
| Roo Code | `~/.roo/skills` | `.roo/skills` |

如果你的平台支持 `.agents/skills`，优先使用它以获得更好的跨平台兼容性。

</details>

> [!TIP]
> 核心工作流仅依赖 Markdown 和 shell 脚本——除了 agent 本身提供的能力外，无需额外运行时依赖。可选的 Python scanner 能提供更深入的分析，但不是必需的。

## 快速开始

告诉你的 agent 你想做什么：

```text
Write a README for this repository.
```

```text
Improve my current README without rewriting it from scratch.
```

```text
Audit this README and tell me the top issues first.
```

```text
Generate README.md and README-ZH.md for this package.
```

skill 随后会：

1. 扫描仓库，收集真实证据（包元数据、入口文件、配置项、已有文档）
2. 对项目类型、分发姿态、受众和风格进行分类
3. 当代码本身无法揭示意图时，提出针对性问题（Quick Mode 下跳过此步）
4. 仅基于已验证的事实来起草或改进 README
5. 校验命令、import 路径、链接、配置一致性和证据完整性

## 工作原理

```mermaid
flowchart LR
    R[Route] --> S[Scan]
    S --> U[Understand]
    U --> I[Interview]
    I --> P[Plan]
    P --> G[Generate]
    G --> V[Verify]

    style R fill:#6366f1,color:#fff,stroke:none
    style S fill:#8b5cf6,color:#fff,stroke:none
    style U fill:#a855f7,color:#fff,stroke:none
    style I fill:#c084fc,color:#fff,stroke:none
    style P fill:#d8b4fe,color:#1a1a2e,stroke:none
    style G fill:#22c55e,color:#fff,stroke:none
    style V fill:#16a34a,color:#fff,stroke:none
```

| 阶段 | 执行内容 |
|---|---|
| **Route** | 选择交付模式：协作起草、Quick Mode、定向改进 或 仅审计 |
| **Scan** | 从源码中采集事实——包元数据、入口文件、配置、文档、git 状态、证明素材 |
| **Understand** | 对项目类型、分发姿态、成熟度、受众和风格进行分类 |
| **Interview** | 提出 3-5 个代码无法回答的针对性问题（Quick Mode 下跳过） |
| **Plan** | 根据分类结果构建章节计划，而非套用通用模板 |
| **Generate** | 逐章节起草或改进 README，以真实证据为依据 |
| **Verify** | 执行 13 项质量检查，覆盖清晰度、准确性、可复制性和完整性 |

## 差异化特性

### 分类驱动，而非模板填充

skill 将四个独立维度分开处理，而不是把所有项目塞进同一个标签：

| 维度 | 选项 |
|---|---|
| **交付模式** | Collaborative Drafting, Quick Mode, Surgical Improvement, Audit Only |
| **项目类型** | Library, CLI, Web App, Framework, API, Extension, Mobile, Research, DevOps, Agent/AI, Internal, Monorepo |
| **受众** | Evaluator, New User, Power User, Contributor, Operator, AI Agent |
| **风格** | Developer Utility, Product, Academic Authority, Community Narrative |

面向评估者的 CLI 工具，和面向学术界的研究仓库，或面向终端用户的扩展插件，会得到完全不同形态的 README。

### 证据优先的写作

skill 使用证据阶梯——声明必须有对应层级的证据支撑：

1. **仓库原生证据**：源码、测试、示例、配置、CI、release tags
2. **用户提供的证据**：定位描述、截图、用户提供的指标
3. **外部可验证证据**：registry 统计、商店评分、benchmark 页面
4. **未验证信息**：省略，或明确标注为假设

它不会编造安装命令、伪造代码示例、臆测功能特性，或生成虚假的社会化证明。

### 分发姿态感知

在写安装说明之前，skill 会先判断项目是 **已发布的包**、**源码优先的仓库**、**框架 + playground 混合体**，还是 **产品级应用**。这能避免 README 中最常见的谎言：为一个从未发布过的项目写上 `npm install <package>`。

### 改进，而非仅仅生成

四种交付模式覆盖了完整的生命周期：

- **Collaborative Drafting** — 通过简短的澄清问题，量身定制 README
- **Quick Mode** — 即时生成初稿，明确标注假设
- **Surgical Improvement** — 对现有 README 进行定向修复
- **Audit Only** — 诊断并排列优先级，不做修改

## README 风格

<table>
<tr>
<th>风格</th>
<th>适用场景</th>
<th>风格方向</th>
</tr>
<tr>
<td><strong>Developer Utility</strong></td>
<td>SDK、CLI、库、开发工具</td>
<td>Badge、一行安装命令、最小可运行示例、精炼亮点</td>
</tr>
<tr>
<td><strong>Product</strong></td>
<td>应用、扩展、面向消费者的产品</td>
<td>视觉优先、干净的产品页风格，适当展示商店链接和社会化证明</td>
</tr>
<tr>
<td><strong>Academic Authority</strong></td>
<td>研究仓库、论文、benchmark</td>
<td>论文链接、benchmark 表格、引用块、可复现性</td>
</tr>
<tr>
<td><strong>Community Narrative</strong></td>
<td>大型社区项目、生态系统工具</td>
<td>品牌故事、路线图、社区频道、贡献者墙</td>
</tr>
</table>

当信号模糊时，skill 默认采用 Developer Utility 风格。

## 验证

每份生成的 README 都会经过 13 项质量检查：

| 检查项 | 捕获的问题 |
|---|---|
| 3 秒测试 | 陌生人不翻页就能理解项目吗？ |
| 复制粘贴测试 | 安装/配置命令能直接跑通吗？ |
| 独立测试 | 新读者能从零到第一次成功运行吗？ |
| 扫读测试 | 仅靠标题和代码块能讲清楚故事吗？ |
| 准确性测试 | 示例是否与当前行为一致？ |
| 公共接口测试 | import 路径用的是公共 API 还是内部路径？ |
| 配置一致性测试 | README 与 `.env.example` 及示例配置一致吗？ |
| 分发姿态测试 | README 与项目的实际消费方式匹配吗？ |
| 时效性测试 | README 反映的是仓库当前状态吗？ |
| 链接完整性测试 | 所有本地链接、图片和文档路径都存在吗？ |
| 证据完整性测试 | 外部证明信号是真实可验证的吗？ |
| 完整性测试 | 该项目类型预期的章节都存在吗？ |
| 语气一致性测试 | 全文写作风格是否保持一致？ |

## Scanner

可选的 `scripts/scan-project.sh` 会执行只读分析，为 skill 的理解阶段提供输入：

```bash
bash scripts/scan-project.sh /path/to/target-repo
```

分析报告包含：

- 项目身份（包元数据、脚本、版本号）
- 语言构成（文件数量和字节估算）
- 分发信号（入口文件、构建配置、registry 指标）
- 公共导出接口（对于库——实际导出了哪些符号）
- 配置接口（环境变量、示例配置文件）
- README 完整性信号（失效的本地引用、不匹配的 import、环境变量漂移）
- Git 元数据（remote、分支、提交数、tags）

> [!NOTE]
> scanner 需要 `python3`（3.10+）才能进行完整分析。没有 Python 时，会回退到 shell 模式，仅报告基本信号。即使没有 scanner，skill 也能正常工作——它会回退到手动检查。

## 仓库结构

```text
readme-crafter-skill/
├── SKILL.md                          # Core workflow — the portable skill definition
├── agents/
│   └── openai.yaml                   # Optional UI metadata for Codex/Cursor surfaces
├── scripts/
│   ├── scan-project.sh               # Entry point — delegates to Python scanner
│   ├── scan-project.py               # Main scanner logic
│   ├── scan_project_support.py       # Inventory, language detection, export analysis
│   └── scan_project_readme.py        # README-specific parsing (imports, env vars, refs)
├── references/
│   ├── style-guide.md                # Per-temperament structure, badges, tone guidance
│   ├── repo-integrity.md             # Distribution posture checks, hard-compare matrix
│   ├── worked-examples.md            # Decision logic examples for ambiguous repos
│   └── quality-checklist.md          # 13-point verification checklist
└── LICENSE
```

## Agent 跨平台兼容性

核心工作流定义在 `SKILL.md` 中，仅使用纯 Markdown 和标准 shell 工具。它可在任何兼容 [Agent Skills](https://agentskills.io) 的平台上运行：

- **Claude Code** — 原生 skill 支持
- **OpenAI Codex** — 通过 `agents/openai.yaml`
- **Cursor** — skill 目录支持
- **Gemini CLI** — skill 目录支持
- **OpenCode** — skill 目录支持
- **Goose, Roo Code, Junie, Amp** — 以及其他兼容的 agent

不依赖任何平台特有的 UI 功能。可选的 `openai.yaml` 为支持它的平台提供展示元数据，但没有它 skill 也能正常运行。

## 它不会做的事

- 把 README 变成完整文档——它会链接到更详细的文档
- 把 AI 专属章节置于主安装路径之上（除非项目本身面向 agent）
- 编造仓库无法支撑的可信度信号
- 把每个项目都当成产品着陆页来写
- 猜测安装命令或包的发布状态

## 参与贡献

欢迎提 Issue 和 PR。

最有价值的贡献是真实世界的案例：

- 生成的 README 效果特别好的仓库
- 风格或受众推断出错的仓库
- 需要更好模式的项目类型
- 质量检查清单未能覆盖的边界情况

## 许可证

[MIT](./LICENSE)
