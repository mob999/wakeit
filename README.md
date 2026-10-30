# Wakeit

English | [简体中文](README-zh.md)

Schedule Agent tasks on your own machines, and read their results in one place.

Wakeit connects the Agents you already use—such as Codex, Claude Code, Kimi Code and Pi—to scheduled work. A **Gateway** hosts the WebUI, schedules tasks and keeps execution history. A **Runner** on each device executes tasks using locally installed Agents. Devices connect outbound; no inbound ports are needed.

## What you can do

- Run tasks manually, once, at fixed intervals or with Cron.
- Connect multiple Agents in a DAG and pass results between dependent steps.
- Create tasks from reusable presets with configurable parameters.
- Follow progress, read reports and continue supported Agent sessions.
- Manage devices, tasks and access across separate workspaces.

Runner supports Linux, macOS and Windows. See [supported Agents and capabilities](docs/agents.md).

## Get started

### 1. Have a Gateway ready

Use an existing Gateway, or follow the [deployment guide](docs/operations.md) to host your own with Docker Compose. For a native installation, see [Gateway services](docs/gateway-service.md).

### 2. Connect your device

Install and sign in to your preferred Agent on the device. Then install the Wakeit CLI on macOS or Linux GNU:

```sh
curl -fsSL https://raw.githubusercontent.com/mob999/wakeit/main/install.sh | sh
```

Open a new terminal and connect to your Gateway. Replace `WORKSPACE_ID` with the workspace UUID in its WebUI URL, and choose the directory where tasks may run:

```sh
wakeit setup --gateway https://wakeit.example.com \
  --workspace WORKSPACE_ID --allow-root /path/to/projects
```

Approve the request in your browser. Setup registers the Runner and configures user-level autostart. [Windows and other installation options →](docs/install.md)

You can also open **Agent Prompt** in the WebUI and give it to your Agent to complete setup. Install or update the [Wakeit skill](docs/skills.md) so your Agent can manage tasks and runs:

```sh
wakeit skill install
```

### 3. Create a task

In the WebUI, click **+** and choose a task, DAG or preset. Select the device, Agent and working directory, enter instructions, and set the schedule. Save it or run it immediately; progress and results appear in Runs.

For CLI workflows, start with the [task example](examples/task.yaml) and [CLI guide](docs/cli.md).

## Update

```sh
wakeit update
```

Restart the Runner after updating. Gateway/WebUI and CLI/Runner are released independently; update your Gateway using its [deployment method](docs/operations.md).

## Documentation

| Topic | Guide |
|---|---|
| Installation and deployment | [CLI](docs/install.md) · [Gateway](docs/operations.md) |
| Tasks, runs and automation | [CLI](docs/cli.md) · [DAGs](docs/dag.md) · [Presets](docs/templates.md) |
| Agents and custom adapters | [Supported Agents](docs/agents.md) · [Plugins](docs/plugins.md) · [Adapter protocol](docs/protocol.md) |
| Teams and access | [Workspaces](docs/workspaces.md) |
| Development and integrations | [Contributing](CONTRIBUTING.md) · [REST API](docs/api.md) |
| Release notes | [Gateway](CHANGELOG.md) · [CLI](CHANGELOG.cli.md) |

[Apache-2.0](LICENSE) · [Third-party components](docs/third-party.md)
