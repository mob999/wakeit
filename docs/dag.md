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

A failed, timed-out, interrupted or skipped predecessor causes dependent nodes to skip; independent branches continue. Nodes may opt into automatic retries with `max_retries`. Trigger and cancel address the whole graph. Whole-graph retry uses the **current Task definition**, creates a new Run and starts every root. Manual node retry keeps the same parent Run and its frozen definition, retaining successful nodes and resuming affected descendants after AND dependencies succeed. Editing a Task never changes an existing Run's snapshot.

Prompts can explicitly reference ancestor results:

```text
Use this analysis:
{{ nodes.collect.result }}
```

Configure `pre` explicitly first; inserting a result reference does not create a dependency. Only the reserved `nodes.<id>.result` form is interpreted. String results are inserted verbatim, structured results are formatted as JSON, and inserted text is not interpreted again. Results must come from the same round. Missing, empty, expired, incomplete or persistently truncated results fail the consuming node before submitting to an Agent. The expanded instruction limit is 1 MiB.

A DAG prompt may also be structured as `{parts:[{text:"literal"},{result:"ancestor-id"}]}`. Text parts never enter the legacy parser, even when they contain reference-looking examples; result parts retain order and read only concrete ancestors from the same Run. Existing string prompts and historical snapshots keep their original behavior. [Task templates](templates.md) compile typed selectors into this form before Task creation, rather than rendering MiniJinja during execution. Runners still receive an ordinary version-1 execution string.

Omit `dag.results` to select all leaves automatically, or specify a nonempty list of node IDs. Reports follow definition order and keep each output's format. Wakeit does not call another Agent to summarize them or turn logs into reports.

## State and lifetime

The parent waits for every node to settle. All successful nodes produce `succeeded`. If all selected outputs succeed but another branch fails or skips, the parent is `warning` (Warning). Any unsuccessful selected output makes it `failed`; AND never bypasses an unsuccessful predecessor. Whole-graph cancellation or timeout takes precedence once all active processes have confirmed termination. `unknown` stays nonterminal, with no completion time, until the actual uncertain nodes are reconciled.

The graph total time limit is disabled by default: omit `execution.timeout` or set it to `null`. Set a duration to enable it; saved explicit limits remain in effect. Ordinary Task and individual node timeouts still default to one hour. The graph clock starts at the first claim and includes retry backoff. A retry in an active Run does not extend it; reopening a settled Run starts a new full window at the next first claim while preserving its original `started_at`. Allowed delay is measured from each node becoming ready. Whole-graph overlap defaults to skip. Session locks apply across all Tasks on the same Runner/Agent/Session, including nodes; unknown executions retain their locks.

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

Migrations `0003_dag.sql` and `0004_task_editor_layout.sql` append to the published 0.2.0 baselines. They preserve ordinary Task/Run IDs and device credentials. Gateway owns graph scheduling; Runners receive ordinary version-1 assignments, so existing Runners remain compatible. Child execution IDs remain for diagnostics and continuation; retry through the parent/node endpoint, never the generic child trigger/retry/cancel endpoints.

## Local acceptance

Build the frontend and Rust binaries, then run the disposable two-Gateway stack:

```sh
cd web && npm ci && npm run build && cd ..
cargo build --examples --bin wakeit
python3 scripts/integration_workspaces.py --keep-running
```

The script creates its own PostgreSQL, NATS, Gateways, test accounts, CLI profiles and Runners under ignored `.local/workspaces-*`. Its summary records ports, process IDs and checks; private test credentials are separate. Omit `--keep-running` for automatic service shutdown. Add `--old-runner /absolute/path/to/previous/wakeit` to verify legacy Runner assignment compatibility. Never point it at an existing user database.

Use `python3 scripts/integration_workspaces.py --templates-only` for focused creation-time template acceptance on a fresh stack. It covers compilation without writes, atomic Create & run, structured result provenance, cross-Gateway idempotency/concurrency, rollback, permissions, frozen graph recovery and CLI schedule semantics. With `--keep-running`, `WAKEIT_TEST_STACK=<stack>/summary.json npm --prefix web test -- tests/templates.spec.ts` additionally exercises the real template UI with synthetic data.

Local browser fixtures in `web/tests/dag-editor.spec.ts`, `dag-workbench-ux.spec.ts`, `dag-canvas.spec.ts` and `dag-runs.spec.ts` use synthetic data and do not run in CI. Set `WAKEIT_TEST_URL` to the Vite address. Screenshots must not contain personal accounts, paths, credentials or actual user output.

## Manual node retry and recovery

```sh
wakeit run retry <parent-run-id> --node <node-id>
wakeit run retry <parent-run-id> --node <node-id> --resume-stopped
```

A confirmed failed, timed-out or interrupted node can be retried after all predecessors succeed. Successful nodes are never repeated. One manual action creates one attempt and does not consume or reset automatic retry quota; failure of that manual attempt does not schedule another automatic attempt. An already queued automatic successor is reused and made ready immediately. The affected downstream nodes resume only when every predecessor succeeds, keeping their own automatic policies. Other failed branches and actual failed descendants require their own explicit retry.

Cancelled or explicitly timed-out graphs require `--resume-stopped` and confirmation that all old executions have stopped. Reconcile unknown executions first. Recovery covers the selected node and descendants skipped for dependency failure or cancelled by the graph stop; it does not start unrelated cancelled branches. Archived Tasks and overlap conflicts reject recovery; paused Tasks allow it. Missing, truncated, expired or incomplete retained inputs/outputs reject recovery instead of recomputing successful nodes. Bound Session occupancy is checked transactionally; a claim-time race leaves a manual attempt failed and eligible for another explicit retry rather than permanently skipped.

`POST /api/v1/workspaces/{workspace}/runs/{parent}/nodes/{node}/retry` requires `Idempotency-Key` and `{ "execution_id": "<current UUID>", "resume_stopped": false }`. `execution_id` is required, but must be `null` if no execution was created. Stale attempts return 409; the same key/body returns the same `{id,parent_run_id,node_id}`. Node detail adds `retry.action`, a blocking `retry.reason` and affected descendants. The CLI reads the latest node before submitting the execution guard. Successful node IDs, results, logs, Trace, Sessions and times remain unchanged. New attempts have independent history and actual input execution IDs in `execution_snapshot.input_executions`.

The WebUI shows Retry node, Retry now or Resume node in the current node context. Historical attempts cannot execute actions. Stopped recovery confirms the selected node and affected descendants in a compact table; operation errors use Toasts. Run lists, search and Moments retain one parent record. A completed Watch stays exited; a new Watch observes the same recovered Run and ignores historical terminal events when determining completion. Without `--node`, `run retry` retains whole-graph retry behavior.

For supplemental restart/cancellation-race checks after `--keep-running`, use `python3 scripts/dag_retry_lifecycle_checks.py --stack .local/workspaces-<id>/summary.json`. It only accepts a disposable stack created by this checkout and restarts that stack's test Gateway. Its summary records the replacement process ID for cleanup. Synthetic 0006-to-0007 migration backfill checks run with the workspace suite.

For a live upgrade regression, prepare a separate old service with `python3 scripts/integration_workspaces.py --keep-running --prepare-only --gateway-binary /absolute/path/to/previous/wakeit`, then run `python3 scripts/dag_retry_live_upgrade_checks.py --stack .local/workspaces-<id>/summary.json`. This only accepts a disposable stack from this checkout. It verifies an active execution, original clock and Runner credentials survive the additive 0007 migration without restarting Runners or repeating Agent calls.
