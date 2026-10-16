# Cline CLI fixture

Synthetic fixture for the current Cline CLI (`cline --json`), checked 2026-10-16. No user content or credentials.

- Official CLI contract: https://github.com/cline/cline/blob/main/docs/cli/cli-reference.mdx#json-output-for-agent-runs
- JSON envelope emission: https://github.com/cline/cline/blob/main/apps/cli/src/utils/events.ts
- Public content/tool event fields: https://github.com/cline/cline/blob/main/sdk/packages/shared/src/agents/types.ts
- Authoritative `run_result` and exit handling: https://github.com/cline/cline/blob/main/apps/cli/src/runtime/run-agent.ts

Repeated `content_start` carries text chunks and an optional authoritative `accumulated` snapshot. Tool events correlate by `toolCallId`. Hidden reasoning is excluded. `agent_event.done` is not the final CLI report: only `run_result` with `finishReason: completed` supplies it. A later `run_aborted` invalidates that report. Native nonzero exit remains authoritative. Cline currently defaults to auto approval in one-shot mode; the adapter does not broaden it further. Models are manual; native session discovery and exact persisted inspection are not declared.
