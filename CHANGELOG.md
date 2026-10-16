## 0.1.1

### Added

- Switch the WebUI between English and Chinese, with a remembered language preference.
- Use built-in adapters for Kimi Code, Antigravity CLI, Gemini CLI, Cursor, GitHub Copilot CLI, Cline, Goose, and Qwen Code, with capabilities exposed according to each native interface.
- Open localized release notes by clicking the version number.

### Improved

- The English Agent Prompt guides CLI setup and installation of the Wakeit skill.
- Agent details use consistent tables and official vector icons. Creation actions use plus icons, Run retry is distinct from refresh, and task selection omits undetected Agents.

### Fixed

- Kimi streams live execution activity and persists titles for new task sessions; existing sessions retain their title and context when resumed.
- Copilot exposes JSONL execution events, Cline receives literal task instructions reliably, and Antigravity detection accepts native help on stderr.
- Codex and Claude publish the final report once, after successful completion.
- Public release checks work without GitHub authentication.

## 0.1.0

- Schedule Agent tasks with cron, fixed intervals, one-time execution, or manual triggers.
- Run tasks on registered devices with configurable working directories, Agent parameters, and model selection where supported.
- View live execution activity, logs, and Markdown results; browse completed runs in the Moments timeline.
- Bind tasks to existing sessions and continue conversations with supported Agents.
- Manage members, API keys, devices, and archived tasks from the WebUI, with administrator, operator, and viewer roles.
- Connect the CLI and register a Runner through browser authorization, with user-level autostart and built-in Agent adapters.
