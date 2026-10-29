## 0.4.1

- Simplified documentation and Agent setup instructions.

## 0.4.0

### Changed

- Gateway and the CLI are now installed and updated separately. Gateway keeps the existing version line; the CLI, including Runner, starts at 0.1.0.
- Deploy the server with `wakeit-gateway`, the Gateway installation script or Docker Compose. Gateway commands are no longer part of `wakeit`; existing service commands must be updated.
- Agent Prompt installs the CLI independently of the Gateway version.

See the [Gateway deployment guide](https://github.com/mob999/wakeit/blob/gateway-v0.4.0/docs/gateway-service.md) for the new installation commands.

## 0.3.4

### Added

- Search and select workspace presets in the task creation window, then preview a task after filling in parameters. Lists and conditions can generate DAG steps and dependencies; created tasks remain independent of later preset changes.
- Install Wakeit or its Agent Skill with a separate single command on macOS and GNU Linux, with installation progress shown.

### Improved

- Ordinary tasks and DAGs share a creation entry point and can be selected in the same window.
- Ordinary and DAG runs share retry and session-continuation actions. For DAGs, choose a node or retry all nodes.
- Agent Prompt provides separate Wakeit and Skill installation commands to copy to an Agent for setup.

See the [upgrade guide](https://github.com/mob999/wakeit/blob/v0.3.4/docs/operations.md#upgrading-from-033) for installation updates.

## 0.3.3

### Improved

- Read reports with separate outline, content and reference columns. The outline and references remain accessible while scrolling, and either sidebar can be resized.
- View Output, Trace and Details separately on run pages, keeping the selected report and reading position when switching tabs.
- Task overviews bring together scheduling, instructions, execution settings and recent runs. Select a DAG node to see its details.
- Preview and copy the complete Agent Prompt in a dedicated window.

### Fixed

- The same image no longer appears more than once in report references.
- Collapsing a Moments report returns to its original card when still reading that report, or keeps the current position after moving to another card.
- Fixed failures when archiving tasks while an execution was being updated.

See the [upgrade guide](https://github.com/mob999/wakeit/blob/v0.3.3/docs/operations.md#upgrading-from-032) for installation updates.

## 0.3.2

### Added

- Retry a failed DAG node while keeping successful results, then continue affected downstream nodes.
- Resume selected nodes and their affected downstream nodes in stopped DAGs, and optionally set a maximum duration for the entire DAG.
- View local images inside reports and file previews, and open the original image.

### Improved

- Switch between outputs in Moments and run results, browse outlines and references, and copy the selected report.
- See why execution activity was omitted, could not be saved or has expired.

### Fixed

- Fixed otherwise healthy Kimi, Copilot, Qwen, Cursor and other supported Agent runs being marked as failed when activity recording or output previews reached their limits.
- File previews no longer close during background refreshes or reconnections.
- Fixed incomplete Chinese and other text when reading logs immediately after execution finished.

See the [upgrade guide](https://github.com/mob999/wakeit/blob/v0.3.2/docs/operations.md#upgrading-from-031) for installation updates.

## 0.3.1

### Added

- Continue supported Agent sessions in a browser terminal, copy their launch commands and reconnect after brief interruptions. Closing the terminal preserves the original session.
- Automatically retry failed DAG nodes, inspect previous attempts and start a queued retry immediately.
- Preview text, code, Markdown, JSON and images linked from reports.

### Improved

- Collapse tool inputs and outputs separately in execution history. New messages no longer interrupt reading earlier content.
- Jump directly to a DAG node with a problem and view its failure reason.
- Settings remembers its sidebar width separately.

See the [upgrade guide](https://github.com/mob999/wakeit/blob/v0.3.1/docs/operations.md#upgrading-from-030) for installation updates.

## 0.3.0

### Added

- Create DAG tasks with multiple steps, parallel execution, dependencies, references to earlier results and a choice of final outputs.
- Drag Agents onto a canvas, connect steps and edit their settings. Search, undo/redo, layout organization, a table view, zoom and fullscreen help manage larger tasks.
- View each DAG node's activity, logs, results and sessions. Successful output remains available when an independent branch fails.

### Improved

- Switch between task settings without completing the current page first.

### Fixed

- Fixed setup reporting success when the Wakeit command was still unavailable in some environments.

See the [upgrade guide](https://github.com/mob999/wakeit/blob/v0.3.0/docs/operations.md#upgrading-from-020) for installation updates.

## 0.2.0

### Added

- Create and join multiple workspaces, each with its own tasks, devices, members and run history.
- Sign in with GitHub or explore a read-only demo workspace. Self-hosted services continue to support password sign-in and invitation-based registration.
- Use `wakeit run watch` to follow a run's status, logs, activity and final result live.

### Improved

- Member permissions clearly distinguish Admin, Read/write and Read-only access.

### Changed

- CLI connections and API keys are scoped to a workspace. Update the CLI when upgrading the service.
- Workspace administrators manage their workspace's members; global account management requires separate instance administrator permissions. Existing resources remain in the Default workspace.

See the [upgrade guide](https://github.com/mob999/wakeit/blob/v0.2.0/docs/workspaces.md#upgrading-an-existing-service) for installation updates.

## 0.1.1

### Added

- Switch between English and Chinese, with your language choice remembered.
- Added support for Kimi Code, Antigravity CLI, Gemini CLI, Cursor, GitHub Copilot CLI, Cline, Goose and Qwen Code.
- Click the version number to read release notes in English or Chinese.

### Improved

- Copy Agent Prompt to an Agent to connect the CLI and install the Wakeit Skill.
- Task creation only lists Agents detected on the device.

### Fixed

- Codex and Claude no longer submit duplicate final results.
- Checking public releases no longer requires GitHub sign-in.

## 0.1.0

- Schedule Agent tasks with cron, fixed intervals, a specific time or manual triggers.
- Run tasks on registered devices with configurable working directories, Agent parameters and model selection where supported.
- View live execution activity, logs and Markdown results, and browse completed runs in the Moments timeline.
- Bind tasks to existing sessions and continue conversations with supported Agents.
- Manage members, API keys, devices and archived tasks from the browser, with administrator, operator and viewer roles.
- Connect the CLI and register devices through browser authorization, with automatic Runner startup after sign-in.
