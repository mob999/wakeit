# Gemini CLI protocol evidence

Verified on 2026-10-16 against the official stable release
[v0.64.0](https://github.com/google-gemini/gemini-cli/releases/tag/v0.64.0).
The JSONL fixture is synthetic; it contains no user conversation or credentials.

- [Headless mode](https://github.com/google-gemini/gemini-cli/blob/v0.64.0/docs/cli/headless.md)
  documents JSONL events and non-TTY / `--prompt` headless execution.
- [CLI reference](https://github.com/google-gemini/gemini-cli/blob/v0.64.0/docs/cli/cli-reference.md)
  documents that `--prompt` is appended to stdin, forces non-interactive mode,
  and that `--model` accepts native model names. The adapter supplies an empty
  explicit prompt and sends the task through stdin.
- [Output event types](https://github.com/google-gemini/gemini-cli/blob/v0.64.0/packages/core/src/output/types.ts)
  define `init`, `message` with optional `delta`, `tool_use`, `tool_result`,
  warning/error severity, and terminal `result` success/error status.
- [Headless event producer](https://github.com/google-gemini/gemini-cli/blob/v0.64.0/packages/cli/src/nonInteractiveCli.ts)
  emits assistant content as deltas and correlates tool events with `tool_id`.
  The terminal result contains statistics, so the adapter retains the final
  assistant turn's text as the answer. Text before tool execution is commentary.
- [Headless policy](https://github.com/google-gemini/gemini-cli/blob/v0.64.0/packages/core/src/scheduler/policy.ts)
  rejects a tool requiring interactive approval in non-interactive mode.
  The template preserves the installed approval policy.

No native model catalog is exposed by this adapter. There is no static list or
fallback. A model name can be supplied explicitly using the native CLI option.

The adapter records the ID from `init` but does not advertise session list,
inspection, or resume. Official Gemini CLI history is project scoped, and the
stream's session ID alone does not establish disk persistence or verify the
exact working directory. Native [session management](https://github.com/google-gemini/gemini-cli/blob/v0.64.0/docs/cli/session-management.md)
supports UUID resume, but wakeit's stricter persistence and directory checks
would require a separate verified metadata adapter.

The packaged `web/public/agent-logos/gemini-cli.svg` is the original vector
[full-color icon](https://geminicli.com/brandassets/gemini-cli-icon_full-color.svg)
linked by the official [brand kit](https://geminicli.com/brand-kit/). Its SVG
geometry and viewBox are unchanged.
