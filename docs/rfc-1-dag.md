# RFC-1: AND DAG tasks

A DAG is one Task; a round is one Run. Nodes are inline execution configurations, not independently runnable Tasks. A complete definition may have multiple roots. Every trigger starts the complete graph; all roots become ready together, subject to Runner capacity and session locks. There is no OR condition, partial start, automatic node retry, subgraph or standalone Task reference.

## Execution and results

All predecessors must succeed. An impossible descendant is skipped while independent branches continue. Each round freezes the definition, output selection and node configuration. Retry uses the current Task definition and starts a new complete round.

Outputs default to the graph's leaves. Explicit output selections must contain at least one valid unique node. Wait for all nodes to settle before aggregation:

- `succeeded`: every node succeeded.
- `warning`: all selected outputs succeeded, but another node failed, timed out, was interrupted or skipped.
- `failed`: a selected output did not succeed.
- Whole-graph cancellation or timeout takes precedence after all executions have confirmed their end.
- `unknown` remains nonterminal and retains session occupancy until reconciled.

Warning is a terminal state, displayed as “Completed with warnings” / “完成但有警告”. Watch returns 0 for succeeded and warning; other execution terminals return 1. Results are actual node reports, never reconstructed logs.

Only `{{ nodes.<id>.result }}` references are interpolated, from ancestors in the same round. Strings are inserted verbatim; structured values are JSON. No recursive interpolation or expression evaluation. Missing, expired, incomplete or truncated input fails before Agent submission. Actual rendered instructions are snapshotted.

## WebUI

Both WebUI and `wakeit task apply --file task.yaml` edit the same complete definition through revision-checked, atomic saves. Ordinary tasks retain their existing editor and wire representation.

The full-page DAG editor keeps primary navigation, top-bar breadcrumbs and Task → Time steps. It temporarily replaces the secondary task list. The first step has the task name/output selection, then exactly two panes: graph or node list on the left, selected node form on the right (400px default, resizable). Below 960px available width, list and selected-node configuration become separate single-pane views. The bottom action bar remains visible. Next validates without saving; only Time saves.

Node fields: name, predecessors, instruction/result insertion, Runner, Agent, directory, model; advanced parameters, session and execution timeout. Add, duplicate and delete retain all other drafts. Referenced nodes cannot be deleted until references and explicit output selection are changed. Node IDs remain stable when display names change.

Graph nodes use HTML controls and SVG edges with worker-based ELK layered layout. Layout is derived and never stored as execution configuration. It does not change on instruction edits or status events. No drag-connect, freely placed nodes, decorative grid or animated edges. Graph/list, search, fit and actual-size controls remain keyboard accessible. Layout failure falls back to the node list.

Task overview contains schedule, graph/selected node and recent runs. A DAG is one entry in task/run lists, search, archive and Moments; node executions do not inflate pagination or totals. Run retains Execution / Details. Process precedes Results; succeeded/warning with a report highlights Results once without taking keyboard focus or switching away from Details. Each node retains independent trace/log cursors, session and display state. Warning retains actual failed nodes and an action to inspect them. Moments previews selected reports; continuation targets a concrete node session.

All views use the current neutral palette, 1px borders, 6px corners, transparent icon controls, thin scrollbars and bilingual labels. No duplicate headings, count boxes, implementation notes or real-data screenshots.

## Persistence and interfaces

Append migration(s); never rewrite released baselines. Parent rounds and internal node executions share workspace-scoped references. Atomic node readiness/execution/outbox and unique parent/node identity prevent duplicate starts. Gateway schedules the graph; Runner receives the existing single-execution version-1 payload. All independent node start/retry/cancel API paths are rejected.

Add complete-definition validation with field/node/cycle errors; extend Run/node queries and events with source node identity and reliable 500-record cursors. Maintain current revision checks, permissions, archive/pause behavior, retention and cross-process session locks. Active parent rounds protect required upstream reports from retention.

Graph timeout defaults to 1h from first node claim; node timeout defaults to 1h. Start grace begins when each node becomes ready. Pause cancels unstarted scheduled rounds; archive cancels all unstarted rounds. Started rounds finish using their snapshot. Cancel/timeout stops dispatch and requests cancellation, never pretending unknown executions stopped.

## Acceptance and delivery

Cover single/multiple roots, chains, fan-out/fan-in, cycle paths, references/JSON, revision conflicts, warning, independent failures, cancel/deadlines/unknown, duplicates/recovery, two Gateways, workspaces and ordinary Task/old Runner compatibility. Prototype fixtures include 20/100 nodes, long content, Chinese/English, light/dark and 390/1440/wide viewports. Browser regression stays local.

Use fresh isolated PostgreSQL, NATS, Gateways and mock Runners. Preserve the current service, database, credentials and Agent auth. Product version stays 0.2.0 and configuration/message version 1; no automatic release or rewrite of released changelogs/tags.

Implementation issues: #55 configuration/prototype; #56 persistence/scheduling; #57 editor/workbench; #58 reports/warning/watch; #59 isolated acceptance/documentation.
