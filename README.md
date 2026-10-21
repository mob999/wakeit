# Wakeit

English | [简体中文](README-zh.md)

Run Agent tasks on a schedule, on your own machines.

Define the instructions and timing, and Wakeit sends the task to a Runner on your device. The Gateway manages scheduling, workspace membership and execution history; the WebUI shows progress, logs and results.

- **Use your existing Agents.** Built-in adapters cover Codex, Claude Code, Pi, OpenCode, Kimi Code, Antigravity CLI, Gemini CLI, Cursor, GitHub Copilot CLI, Cline, Goose and Qwen Code. [Capabilities](docs/agents.md) depend on each native interface; custom adapters use command configuration.
- **Execute on your own devices.** Runners connect outbound to the Gateway on Linux, macOS and Windows. Devices need no inbound ports.
- **Schedule work and read the results.** Use Cron, fixed intervals, one-time schedules or manual runs. View live execution traces and Markdown reports with syntax highlighting, math and Mermaid diagrams. Moments collects results on a compact completion-time timeline, with report outlines, linked files and per-DAG output selection. Open supported local file links through the original Runner to preview report files within its allowed directories.
- **Compose Agent DAGs.** Drag Agents onto the canvas or apply one YAML definition, connect AND dependencies and reference upstream results. Run the complete graph with selected output reports and configurable node retries with exponential backoff. See [DAG tasks](docs/dag.md).
- **Keep the conversation going.** Bind tasks to supported native Agent sessions. Continue them in the browser terminal, copy the original command, or use `wakeit run resume <run-id>` on the Runner device. See [session continuation](docs/cli.md#绑定会话与本机续聊).
- **Separate workspaces.** Keep tasks, runs, devices and permissions isolated across teams. See [workspace configuration and authentication](docs/workspaces.md).
- **Host locally or in the cloud.** One Rust binary includes the Gateway, Runner and CLI; the WebUI ships with the Gateway.

The latest published release is **0.3.1**, licensed under Apache-2.0. Use a CLI version that matches your Gateway. APIs and data formats may still change.

Local PNG/JPEG/WebP references render inline in reports and Markdown-file previews through the original Runner; relative image paths use the execution or Markdown directory and existing workspace/root permissions apply.

## Install

Download the binary for your platform from the [v0.3.1 Release](https://github.com/mob999/wakeit/releases/tag/v0.3.1), along with `wakeit-release.json` and `SHA256SUMS`. Verify SHA-256 before installing it into your user bin directory. Linux, macOS and Windows are supported.

To build from source, install Rust 1.89+ and Node.js 22.12+, then run:

```sh
git clone https://github.com/mob999/wakeit.git
cd wakeit
npm --prefix web ci
npm --prefix web run build
cargo install --locked --path crates/wakeit
```

## Connect to a Gateway

With an existing Gateway, click `{}` in the WebUI toolbar to copy the Agent Prompt. Give it to your Agent to install the CLI and Wakeit skill, request browser authorization, register the device and configure user-level autostart. Review and approve the request in your browser.

Replace `WORKSPACE_ID` with the workspace UUID from its WebUI URL:

```sh
wakeit setup --gateway https://wakeit.example.com --workspace WORKSPACE_ID
# Optional: --runner-name dev-server --allow-root /path/to/projects
wakeit connection list
wakeit runner list
```

The default device name is the hostname, and the default execution scope is the current directory. Admin and Read/write members can register Runners; only Admin can revoke them. Setup preserves existing Agent installations and authentication, and does not create or execute tasks.

For CLI access only, use `wakeit login --gateway URL --workspace WORKSPACE_ID`; Read-only members can use this too. Connections are stored by Gateway, workspace and account in the platform's standard configuration directory and work across directories. API Keys have no expiry by default and can be revoked in Settings → API Key. Use `wakeit connection use NAME` to switch connections. Explicit arguments and `WAKEIT_*` environment variables remain available.

Workspace Admins manage memberships and invitations in Settings → Members. Instance administrators manage global accounts, account activation and local password resets through Instance accounts. Disabling an account or resetting its password revokes its sessions and API Keys.

## Host your own Gateway

For a single-host deployment, install Docker Compose v2 and run these commands from the repository root:

```sh
cp .env.example .env
# Set WAKEIT_POSTGRES_PASSWORD in .env to a generated alphanumeric password.
docker compose build
docker compose up -d
export WAKEIT_PASSWORD='replace-with-a-generated-admin-password'
docker compose run --rm -e WAKEIT_PASSWORD gateway \
  gateway bootstrap --config /app/gateway.yaml --username admin
unset WAKEIT_PASSWORD
```

Open <http://localhost:8080> and sign in. Compose persists PostgreSQL, NATS data and device identities, and restarts services automatically. On Linux, enable Docker at system startup as well.

Docker Desktop starting at login on macOS or Windows does not provide startup before a user signs in. For system-level startup, use the [native Gateway service guide](docs/gateway-service.md). For remote devices, configure the Gateway's public URL and the NATS WSS endpoint; see [deployment and operations](docs/operations.md).

## Create your first task

Install and sign in to the Agent you want to use on the Runner device, then run `wakeit setup --gateway URL --workspace WORKSPACE_ID`. Once user-level autostart is installed, you can close the terminal. `--allow-root` defines the permitted execution directories; without an allowed scope, directory browsing and task execution are blocked. Manual enrollment and autostart options are covered in the [Runner guide](skills/wakeit/references/runner.md).

In the WebUI, create a task: enter its name and instructions, choose a Runner, directory, Agent and model, then continue to the Time step to configure the schedule and save.

You can also import the [example task](examples/task.yaml) through the CLI. Adjust its device, directory and instructions first:

```sh
wakeit task apply --file examples/task.yaml
wakeit --json task trigger daily-review
wakeit run watch <run-id>
wakeit task pause daily-review
```

Tasks are stored in the Gateway's PostgreSQL database; YAML files are inputs for creating or updating them. Model discovery depends on the adapter; enter a model manually when no discovery interface is available. Ordinary tasks are not automatically rerun. DAG nodes can opt into bounded retries with `max_retries`; queued retries can be started immediately without adding attempts. An execution whose outcome cannot be confirmed enters `unknown`; check its actual state before deciding whether to retry.

Archive a task from its detail view to stop future scheduling while preserving history and letting active runs finish. Restore it from Settings → Archived tasks; restored tasks remain paused. CLI equivalents are `wakeit task archive <id>`, `wakeit task list --archived` and `wakeit task restore <id>`.

## Let an Agent help you use Wakeit

The repository includes an independently installable [Wakeit skill](skills/wakeit/SKILL.md). Install the entire `skills/wakeit` directory into your Agent's skill directory, then invoke `$wakeit`:

> Connect to my existing Gateway, register this device, and create a task that reviews the repository every day at 09:00.

The skill loads guides for connections, deployment, Runners, tasks, runs, adapters and updates as needed. It distinguishes connecting to an existing service from deploying a new Gateway. See the [skill installation guide](docs/skills.md).

## Update

```sh
wakeit update --check
wakeit update
```

Update checks use public GitHub Releases and require no GitHub sign-in. Regular updates exclude prereleases and compare release versions; a different commit with the same version does not trigger an update. After verification, the CLI replaces only the binary, preserving data and configuration. It does not automatically restart running services.

## Documentation

| Goal | Guide |
|---|---|
| Manage tasks and runs from the CLI | [CLI guide](docs/cli.md) |
| Configure workspaces, membership and GitHub login | [Workspaces](docs/workspaces.md) |
| Configure system startup | [Gateway services](docs/gateway-service.md) |
| Deploy, back up, restore and upgrade | [Operations](docs/operations.md) |
| Develop an Agent adapter | [Configuration and adapter protocol](docs/protocol.md) |
| Call the API | [REST API](docs/api.md) |
| Build and run tests | [Contributing](CONTRIBUTING.md) |

License: [Apache-2.0](LICENSE). See [third-party components](docs/third-party.md) for bundled skills and Windows service tooling.
