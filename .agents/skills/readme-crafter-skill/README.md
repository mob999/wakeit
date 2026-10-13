**English** | [中文](./README-ZH.md)

<div align="center">

# README Crafter

**An Agent Skill that writes honest, well-structured READMEs tailored to each project's type, audience, and distribution model.**

[![License](https://img.shields.io/badge/License-MIT-111827?style=flat-square)](./LICENSE)
[![Agent Skills](https://img.shields.io/badge/Agent_Skills-Compatible-4f46e5?style=flat-square)](https://agentskills.io)
[![Python 3.10+](https://img.shields.io/badge/Scanner-Python_3.10+-3776AB?style=flat-square&logo=python&logoColor=white)](#scanner)
[![Portable](https://img.shields.io/badge/Portable-Shell_%2B_Markdown-22c55e?style=flat-square)](#agent-portability)

</div>

---

Most README generators scan a repo and fill a template. The result reads like a form, not a front page.

README Crafter takes a different approach: it classifies the project along four independent axes — project type, distribution posture, audience, and presentation temperament — then generates a README shaped for that specific combination. It refuses to invent install commands, fabricate code examples, or claim features that don't exist in the source.

## Install

### One-liner (recommended)

Auto-detects your installed agents (Claude Code, Cursor, Codex, Gemini CLI, and 40+ others) and installs the skill to all of them:

```bash
npx skills add linhai0872/readme-crafter-skill
```

Add `--global` to install at user level, or omit for project level. Add `--all` to skip prompts.

### Ask your agent to install it

Copy the prompt below and paste it into any AI coding agent — it will handle the rest:

```text
Install the readme-crafter-skill for me: clone https://github.com/linhai0872/readme-crafter-skill.git
into my skills directory and symlink it so this agent can use it. Verify SKILL.md is accessible after setup.
```

### Manual install

<details>
<summary>Clone + symlink</summary>

```bash
git clone https://github.com/linhai0872/readme-crafter-skill.git
```

Then symlink into your agent's skill directory:

```bash
# Claude Code
mkdir -p ~/.claude/skills
ln -s /path/to/readme-crafter-skill ~/.claude/skills/readme-crafter-skill

# Cross-agent standard
mkdir -p ~/.agents/skills
ln -s /path/to/readme-crafter-skill ~/.agents/skills/readme-crafter-skill
```

| Platform | User-level directory | Workspace directory |
|---|---|---|
| Claude Code | `~/.claude/skills` | `.claude/skills` |
| Codex | `~/.agents/skills` | `.agents/skills` |
| Cursor | `~/.agents/skills` or `~/.cursor/skills` | `.agents/skills` or `.cursor/skills` |
| Gemini CLI | `~/.agents/skills` or `~/.gemini/skills` | `.agents/skills` or `.gemini/skills` |
| OpenCode | `~/.agents/skills` or `~/.config/opencode/skills` | `.agents/skills` or `.opencode/skills` |
| Goose | `~/.config/goose/skills` | `.goose/skills` |
| Roo Code | `~/.roo/skills` | `.roo/skills` |

If your platform supports `.agents/skills`, prefer it for portability.

</details>

> [!TIP]
> The core workflow is pure Markdown and shell scripts — no runtime dependencies beyond what your agent already provides. The optional Python scanner adds deeper analysis but is not required.

## Quick Start

Tell your agent what you need:

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

The skill then:

1. Scans the repo for real evidence (package metadata, entry points, config surface, existing docs)
2. Classifies the project type, distribution posture, audience, and temperament
3. Asks targeted questions when code alone can't reveal intent (or skips this in Quick Mode)
4. Drafts or improves the README using only verified facts
5. Verifies commands, imports, links, config parity, and evidence integrity

## How It Works

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

| Phase | What happens |
|---|---|
| **Route** | Choose delivery mode: Collaborative Drafting, Quick Mode, Surgical Improvement, or Audit Only |
| **Scan** | Collect facts from source — package metadata, entry points, configs, docs, git state, proof assets |
| **Understand** | Classify project type, distribution posture, maturity, audience, and temperament |
| **Interview** | Ask 3-5 targeted questions that code cannot answer (skipped in Quick Mode) |
| **Plan** | Build a section plan shaped by the classification, not a generic template |
| **Generate** | Draft or improve the README section by section, using real evidence |
| **Verify** | Run 13 quality checks covering clarity, accuracy, copy-paste safety, and integrity |

## What Makes It Different

### Classification over templates

The skill separates four independent axes instead of forcing everything through one label:

| Axis | Options |
|---|---|
| **Delivery Mode** | Collaborative Drafting, Quick Mode, Surgical Improvement, Audit Only |
| **Project Type** | Library, CLI, Web App, Framework, API, Extension, Mobile, Research, DevOps, Agent/AI, Internal, Monorepo |
| **Audience** | Evaluator, New User, Power User, Contributor, Operator, AI Agent |
| **Temperament** | Developer Utility, Product, Academic Authority, Community Narrative |

A CLI tool for evaluators gets a different README shape than a research repo for academics or an extension for end users.

### Evidence-first writing

The skill uses a proof ladder — claims must be backed by evidence at the right level:

1. **Repository-native**: source code, tests, examples, config, CI, release tags
2. **User-provided**: positioning, screenshots, metrics supplied by the user
3. **Externally verified**: registry stats, store ratings, benchmark pages
4. **Unverified**: omitted or explicitly marked as assumptions

It will not invent install commands, fabricate code examples, guess at features, or generate fake social proof.

### Distribution posture awareness

Before writing install instructions, the skill determines whether the project is a **published package**, a **source-first repo**, a **framework + playground hybrid**, or a **product application**. This prevents the most common README lie: showing `npm install <package>` for a project that was never published.

### Improvement, not just generation

Four delivery modes handle the full lifecycle:

- **Collaborative Drafting** — tailored README with brief clarification questions
- **Quick Mode** — immediate first draft with explicit assumptions
- **Surgical Improvement** — targeted fixes to an existing README
- **Audit Only** — diagnosis and prioritization without edits

## README Styles

<table>
<tr>
<th>Temperament</th>
<th>Best For</th>
<th>Style Direction</th>
</tr>
<tr>
<td><strong>Developer Utility</strong></td>
<td>SDKs, CLIs, libraries, dev tools</td>
<td>Badges, one-line install, minimal runnable example, concise highlights</td>
</tr>
<tr>
<td><strong>Product</strong></td>
<td>Apps, extensions, consumer products</td>
<td>Visual-first, clean product-page feel, store links and social proof when applicable</td>
</tr>
<tr>
<td><strong>Academic Authority</strong></td>
<td>Research repos, papers, benchmarks</td>
<td>Paper links, benchmark tables, citation blocks, reproducibility</td>
</tr>
<tr>
<td><strong>Community Narrative</strong></td>
<td>Large community projects, ecosystem tools</td>
<td>Brand story, roadmap, community channels, contributor walls</td>
</tr>
</table>

When signals are ambiguous, the skill defaults to Developer Utility.

## Verification

Every generated README is checked against 13 quality criteria:

| Check | What it catches |
|---|---|
| 3-Second Test | Can a stranger understand the project without scrolling? |
| Copy-Paste Test | Do install/setup commands actually work? |
| Solo Test | Can a new reader go from zero to first success? |
| Scan Test | Do headings and code blocks tell the story alone? |
| Accuracy Test | Do examples match current behavior? |
| Public Surface Test | Do imports use the package's public API, not internal paths? |
| Config Parity Test | Does the README match `.env.example` and sample configs? |
| Distribution Posture Test | Does the README match how the project is actually consumed? |
| Freshness Test | Does the README reflect the current repo state? |
| Link Integrity Test | Do all local links, images, and docs paths exist? |
| Evidence Integrity Test | Are external proof signals real and verifiable? |
| Completeness Test | Are the expected sections present for this project type? |
| Tone Consistency Test | Is the writing style consistent throughout? |

## Scanner

The optional `scripts/scan-project.sh` runs a read-only analysis that feeds the skill's understanding phase:

```bash
bash scripts/scan-project.sh /path/to/target-repo
```

What it reports:

- Project identity (package metadata, scripts, version)
- Language breakdown (file counts and byte estimates)
- Distribution signals (entry points, build configs, registry indicators)
- Public export surface (for libraries — what symbols are actually exported)
- Configuration surface (env vars, sample config files)
- README integrity signals (broken local refs, mismatched imports, env var drift)
- Git metadata (remote, branch, commits, tags)

> [!NOTE]
> The scanner requires `python3` (3.10+) for full analysis. Without it, a shell fallback reports basic signals only. The skill works without the scanner — it falls back to manual inspection.

## Repository Layout

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

## Agent Portability

The core workflow lives in `SKILL.md` and uses plain Markdown and standard shell tools. It works across any [Agent Skills](https://agentskills.io)-compatible platform:

- **Claude Code** — native skill support
- **OpenAI Codex** — via `agents/openai.yaml`
- **Cursor** — skill directory support
- **Gemini CLI** — skill directory support
- **OpenCode** — skill directory support
- **Goose, Roo Code, Junie, Amp** — and other compatible agents

No product-specific UI features are required. The optional `openai.yaml` adds display metadata for platforms that support it, but the skill degrades gracefully without it.

## What It Will Not Do

- Turn a README into full documentation — it links to deeper docs instead
- Put AI-only sections above the main install path unless the project is agent-facing
- Invent credibility signals the repo cannot support
- Treat every project like a product landing page
- Guess at install commands or package publication status

## Contributing

Issues and PRs are welcome.

The most useful contributions are real-world examples:

- Repos where the generated README worked especially well
- Repos where the temperament or audience inference was wrong
- Project types that need a better pattern
- Edge cases the quality checklist doesn't catch

## License

[MIT](./LICENSE)
