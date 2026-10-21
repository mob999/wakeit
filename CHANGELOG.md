## 0.3.1

### Added

- Continue supported native Agent sessions in a browser terminal, with original-command copying, terminal resizing and reconnection after a short interruption. Closing the terminal ends its interactive process while preserving the native session.
- Configure bounded DAG node retries with exponential backoff, inspect individual attempts and start an already queued retry immediately without adding attempts.
- Preview supported text, code, Markdown, JSON and image files linked from saved reports through the original Runner, within its permitted directories.

### Improved

- Run history uses compact, collapsible summaries, independent tool input/output folds and JSON syntax highlighting without code-block backgrounds in parameters and results. Reading earlier messages pauses following; new output and ready results no longer interrupt it.
- DAG structure stays bounded above the selected node's activity. Whole-run status and current node issues remain beside the Execution/Details tabs, with failure reasons in tooltips and direct navigation to the selected node's current attempt.
- Secondary sidebars have distinct surfaces, consistent typography and aligned selection states; Settings retains a separate width preference.

### Fixed

- DAG node pickers keep long names, statuses and checkmarks aligned, and execution times use consistent compact typography.

### Upgrade

- Back up PostgreSQL and stop all Gateway writers before applying `0005_terminals.sql` and `0006_dag_retries.sql`. Update Runners to enable browser terminals, file previews and complete failure-retry support. Existing data, device identities and Agent credentials are preserved; configuration and message protocols remain version 1. See the [upgrade guide](https://github.com/mob999/wakeit/blob/v0.3.1/docs/operations.md#upgrading-from-030).

## 0.3.0

### Added

- Define an entire AND DAG in one Task, with multiple roots, parallel branches, explicit upstream-result references and selectable output nodes. Trigger, retry and cancel operate on the whole graph.
- Build DAGs by dragging detected Agents onto a canvas, connecting dependencies and editing nodes in a floating inspector. Search, table view, undo/redo, layout organization and shared saved coordinates support larger graphs.
- Inspect per-node activity, logs, results and sessions from a DAG Run. Moments and CLI Watch show the whole round and its selected reports; warning rounds retain successful output when an independent branch fails.

### Improved

- Settings, node tables and Run details use consistent typography, aligned columns and table-card layouts. Errors appear as top-right Toasts, and Task/Time tabs switch freely while drafts are incomplete.
- Task overview, Run graphs and the editor share upper-right zoom and fullscreen controls.

### Fixed

- Agent-guided setup configures and verifies PATH in the user's actual login shell before reporting completion.

### Upgrade

- Back up the database and stop all Gateway writers before applying the appended DAG and editor-layout migrations. Existing ordinary tasks, run history and Runner identities are preserved; configuration and message protocols remain version 1. See the [upgrade guide](https://github.com/mob999/wakeit/blob/v0.3.0/docs/operations.md#upgrading-from-020).

## 0.2.0

### Added

- Create and join multiple workspaces, with separate tasks, runs, Runners, settings and member permissions in each workspace.
- Sign in with GitHub and explore a synthetic, server-enforced read-only demo workspace. Self-hosted Gateways retain password authentication and invitation-based registration.
- Watch a specific execution with `wakeit run watch <run-id>`, including live status, logs, Agent activity and the final result; supports JSONL output and observation timeouts.

### Improved

- Switch workspaces from the account menu. Settings stays available to all roles, with workspace creation and leaving in Settings and privileged controls restricted to administrators.
- Permission labels use Admin, Read/write and Read-only. User lists show names directly, and the Chinese Runs label is shortened to 运行.
- The navigation includes a GitHub repository link. The primary README is English, with a linked Chinese edition and workspace-aware setup instructions.

### Changed

- Business APIs now require an explicit workspace path, and API Keys and CLI connections are scoped to a Gateway, workspace and account. Update CLI clients when upgrading the Gateway.
- Workspace administrators manage memberships; global account administration requires separate instance privileges. Upgrading preserves existing resources, API Keys and Runner identities in the Default workspace without automatically granting instance privileges to existing administrators.
- Upgrading from 0.1.1 requires the new workspace database migration. Back up and stop old Gateways before migration; existing Runners can keep their identities. See the [upgrade guide](https://github.com/mob999/wakeit/blob/v0.2.0/docs/workspaces.md#upgrading-an-existing-service).

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
