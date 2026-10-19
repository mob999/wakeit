# AND DAG tasks

A DAG Task contains an entire graph. Each trigger creates one parent Run, freezes the graph and its output selection, and starts all roots. Nodes are inline execution configurations, not independently runnable Tasks. Ordinary Tasks retain their existing YAML and behavior.

Apply [the complete example](../examples/task-dag.yaml) after choosing actual Runner names, installed Agent IDs and permitted working directories:

```sh
wakeit --json task apply --file examples/task-dag.yaml
wakeit --request-key research-once --json task trigger daily-research
wakeit run watch <returned-run-id> --timeout 1h
```

The top-level `dag` replaces `runner`, `agent`, `cwd`, `prompt`, `parameters` and `session`. Schedule and execution policies belong to the graph. Each node has a stable `id`, display `name`, optional `pre` array, Runner, Agent, working directory, prompt, parameters, optional Session binding and timeout. IDs stay stable when names change. Saved Tasks cannot change between ordinary and DAG types.

## Dependencies and results

Only AND is supported: a node runs after every predecessor succeeds. No predecessors means a root. Multiple roots become ready together; Runner capacity and Session locks still limit execution. Self-dependencies, duplicate or missing predecessors, cycles and invalid result references are rejected before saving. Validation errors include the node, field and cycle path when applicable.

A failed, timed-out, interrupted or skipped predecessor causes dependent nodes to skip; independent branches continue. Nodes have no automatic retry. Trigger, retry and cancel always address the whole graph. A retry uses the **current Task definition**, creates a new Run and starts every root. Editing a Task never changes an existing Run's snapshot.

Prompts can explicitly reference ancestor results:

```text
Use this analysis:
{{ nodes.collect.result }}
```

Configure `pre` explicitly first; inserting a result reference does not create a dependency. Only the reserved `nodes.<id>.result` form is interpreted. String results are inserted verbatim, structured results are formatted as JSON, and inserted text is not interpreted again. Results must come from the same round. Missing, empty, expired, incomplete or persistently truncated results fail the consuming node before submitting to an Agent. The expanded instruction limit is 1 MiB.

Omit `dag.results` to select all leaves automatically, or specify a nonempty list of node IDs. Reports follow definition order and keep each output's format. Wakeit does not call another Agent to summarize them or turn logs into reports.

## State and lifetime

The parent waits for every node to settle. All successful nodes produce `succeeded`. If all selected outputs succeed but another branch fails or skips, the parent is `warning` (Warning). Any unsuccessful selected output makes it `failed`; AND never bypasses an unsuccessful predecessor. Whole-graph cancellation or timeout takes precedence once all active processes have confirmed termination. `unknown` stays nonterminal, with no completion time, until the actual uncertain nodes are reconciled.

Graph and node timeouts default to one hour. The graph clock starts when the first node is claimed; node timeout can be overridden on that node. Allowed delay is measured from each node becoming ready. Whole-graph overlap defaults to skip. Session locks apply across all Tasks on the same Runner/Agent/Session, including nodes; unknown executions retain their locks.

Pausing cancels scheduled rounds that have not started. Archiving cancels all unstarted rounds and blocks new triggers and retries. Already-started rounds continue with their frozen definitions. Cancelling a graph stops future dispatch and requests cancellation of active nodes; a cancellation request is not proof that a process ended.

## WebUI

Tasks → Plus → DAG task opens a full-height graph workbench without a secondary sidebar. The Task name is editable in the final top-bar breadcrumb on either step. A new draft is empty. Drag an installed Agent from its Runner group onto the canvas, or use Plus and choose an Agent with a click or keyboard. Each item identifies a specific Runner and Agent; unavailable adapters and revoked devices are excluded, while offline devices remain selectable. The same Agent can create multiple independent nodes.

Drag from a node's output port to another node's input port to declare a predecessor. All incoming edges are AND dependencies. Drag to empty space, or use the node's Plus, to choose an Agent for a successor. Invalid cycles, self-connections and duplicate edges are rejected. Connections and the predecessor picker edit the same `pre` list. They do not insert result references into instructions. Nodes can move individually or as a selection; pan, zoom, search and explicit Organize layout preserve an editable canvas. Undo/redo covers graph changes and coordinates without reverting unrelated instruction edits. Text fields retain native undo. The upper-right canvas controls provide zoom and a full workspace editor view; Escape restores the view after closing active overlays.

Clicking a node opens its floating configuration inside the canvas. The panel has its own scrolling body, can be resized on desktop and becomes a full-size overlay below 960px. Blank clicks or Escape close it; panning does not. Instructions can expand into a larger editor within the workbench. The table view uses aligned Node, Predecessors, Runner, Agent and Actions columns and opens the same inspector. Only the selected node runs discovery queries; per-node Session drafts, model mode, caret and scroll position survive switches.

Result settings use the same floating panel, with Automatic leaves or explicit nodes. Explicit output drafts may temporarily be empty or refer to a deleted node; Save requires valid nonempty outputs. Successors inherit a working directory on the same Runner and compatible model parameters and timeout for the same Runner/Agent, with an empty instruction and fresh Session. Copy preserves execution configuration, Session and predecessors under a new ID. Deleting removes incident dependency edges without reconnecting neighbors. A confirmation lists affected instruction/output references; these stay in the draft for explicit repair, can be undone and prevent Save until valid.

The Time step uses matching scheduling and execution-policy cards. Cron, interval and one-time inputs keep separate drafts. One-time input is interpreted in the selected IANA timezone: nonexistent local times are rejected and repeated times use the earlier occurrence. Previews include the actual UTC offset. Switching steps and Next never validate or write a Task; Save validates the complete draft. Validation uses Toasts and stable field markers, while request failures and revision conflicts retain the draft. Per-node editor state and graph viewport survive node and step changes.

Run keeps Execution and Details. Execution has Process followed by Results, with the same graph/table and a white context row for the selected node. Reports are separated by rows inside one card. Successful and warning rounds highlight reports once; Details is never forcibly left. Details is a single node-details card with node selection, state and node actions in its sole header. Logs start expanded and execution information starts collapsed; both use internal white folding rows. Single-node Runs show a name without a picker. Both tabs share node selection and preserve each node's folding and log-follow state. Unknown reconciliation and continuation commands belong to a concrete node execution and its actual Session.

Task overview and Run graphs share the editor’s three upper-right zoom/full-workspace controls. Fullscreen keeps node selection and scale; Escape or a switch to table view restores the normal layout without changing the Task.

Moments has one entry per graph round, with separate output previews sharing a total 2048 Unicode character budget. Internal node executions do not enter ordinary Run lists, pagination, history or timeline counts.

## API and compatibility

All paths below are workspace-scoped under `/api/v1/workspaces/{workspace}`:

- `POST /tasks/validate`: `{config,editor_layout?}` → `{errors:[{path,node_id?,message,cycle?}]}`; read-only, Operator required. PUT validates again and applies the entire definition atomically with `expected_revision`.
- `GET /tasks/{id}` returns `editor_layout` separately from `config`. `PUT /tasks/{id}` accepts `{config,expected_revision,editor_layout?}` and atomically saves the definition and shared node coordinates. Layout version is 1, with `positions` indexed by stable node ID and finite `x`/`y` coordinates. Omitted or null layout preserves remaining positions; removed nodes are pruned, so CLI apply does not erase layout. Ordinary Tasks reject DAG layouts. Viewport, selection and panel width are local UI state.
- `GET /runs/{id}`: frozen parent configuration, selected reports and node summaries.
- `GET /runs/{id}/nodes/{node}`: `{node,execution}`; execution is null before dispatch.
- `GET /runs/{id}/nodes/{node}/events?after=0&kind=trace`: original node execution events, at most 500 per batch.
- `POST /runs/{id}/nodes/{node}/resolve`: `{state,reason}` for a checked unknown execution.

Parent events merge node events in durable ID order. Child events include `node_id`, `execution_id` and `original_seq`; child state events use `node_state`. Read every batch, advance `after` only after processing it, and aggregate Trace by **DAG node ID plus native Trace ID**. Whole-graph `run watch --json` preserves these identities and ends with the parent report. `succeeded` and `warning` exit 0.

Migrations `0003_dag.sql` and `0004_task_editor_layout.sql` append to the published 0.2.0 baselines. They preserve ordinary Task/Run IDs and device credentials. Gateway owns graph scheduling; Runners receive ordinary version-1 assignments, so existing Runners remain compatible. A node's real execution ID is for read-only diagnostics and local continuation, never independent trigger/retry/cancel.

## Local acceptance

Build the frontend and Rust binaries, then run the disposable two-Gateway stack:

```sh
cd web && npm ci && npm run build && cd ..
cargo build --examples --bin wakeit
python3 scripts/integration_workspaces.py --keep-running
```

The script creates its own PostgreSQL, NATS, Gateways, test accounts, CLI profiles and Runners under ignored `.local/workspaces-*`. Its summary records ports, process IDs and checks; private test credentials are separate. Omit `--keep-running` for automatic service shutdown. Add `--old-runner /absolute/path/to/previous/wakeit` to verify legacy Runner assignment compatibility. Never point it at an existing user database.

Local browser fixtures in `web/tests/dag-editor.spec.ts`, `dag-workbench-ux.spec.ts`, `dag-canvas.spec.ts` and `dag-runs.spec.ts` use synthetic data and do not run in CI. Set `WAKEIT_TEST_URL` to the Vite address. Screenshots must not contain personal accounts, paths, credentials or actual user output.
