# Qwen Code and Cursor protocol evidence

Verified against official documentation/source on 2026-10-16. The JSONL files
are synthetic protocol fixtures, not captures from authenticated inference.
No agents were installed or run, and no credentials/configuration were read.

## Qwen Code

- [Official headless guide](https://qwenlm.github.io/qwen-code-docs/en/users/features/headless/)
- [Pinned finalized assistant/tool/result emitter](https://github.com/QwenLM/qwen-code/blob/70758572ff16bfe7b703a25c3e3f8ac7901625b7/packages/cli/src/nonInteractive/io/BaseJsonOutputAdapter.ts)
- [Pinned streaming adapter](https://github.com/QwenLM/qwen-code/blob/70758572ff16bfe7b703a25c3e3f8ac7901625b7/packages/cli/src/nonInteractive/io/StreamJsonOutputAdapter.ts)
- [Pinned CLI message types](https://github.com/QwenLM/qwen-code/blob/70758572ff16bfe7b703a25c3e3f8ac7901625b7/packages/cli/src/nonInteractive/types.ts)
- [Official vector logo](https://github.com/QwenLM/qwen-code/blob/70758572ff16bfe7b703a25c3e3f8ac7901625b7/packages/desktop/bootstrap/qwen-code-logo.svg), copied unchanged to `web/public/agent-logos/qwen-code.svg`

The template uses `qwen --prompt TEXT --output-format stream-json`, with an
optional `--model ALIAS`. It intentionally does not enable partial-message
output. Finalized assistant `message.content[]` contains text, thinking, or
`tool_use`; user messages carry `tool_result` blocks with native `is_error`.
The final result identifies success/error; its string is authoritative.
Nonzero native exit codes remain failures even if an earlier message exists.
The guide documents turn-limit exit 53, budget exit 55, and SIGINT exit 130.
The adapter does not remap these native exit codes to success.

Thinking is omitted. Native tool IDs and parent-tool IDs are preserved.
A tool error may be recovered by a later successful turn; a failed terminal
result cannot be overwritten by a late success. Missing final result,
malformed required fields, unmatched results, or unfinished calls fail closed.

Qwen is the CLI's executable name; Qwen Code is the coding agent. This does
not integrate a raw Qwen model/API or infer capabilities from Gemini CLI even
though older Qwen Code releases originated from that project. The native
protocol contains a `get_available_models` control request, but starting its
persistent transport has not been verified here to avoid configuration/runtime
side effects. No model catalog is advertised. The model alias remains editable.
Native resume flags alone do not establish exact, read-only metadata inspection,
so the template does not advertise session list/inspect/resume.

## Cursor

- [Official output schema](https://cursor.com/docs/cli/reference/output-format)
- [Official headless guide](https://cursor.com/docs/cli/headless)
- [Official CLI parameter reference](https://cursor.com/docs/cli/reference/parameters)
- [Official installer](https://cursor.com/install), inspected only as text

The current official Unix installer inspected on 2026-10-16 targets
`2026.10.14-941492e`. It explicitly creates both the primary `agent` command
and legacy `cursor-agent` alias pointing to the same native launcher. The
Wakeit template selects `cursor-agent` to avoid matching an unrelated generic
`agent` program. The editor's `cursor` launcher is a different command.
The installer was not executed; no installation compatibility is inferred
from an installed local Cursor binary.

The template selects `--print --output-format stream-json` and supplies the
prompt as one positional argument, with optional `--model`. Without
`--stream-partial-output`, assistant events are complete message snapshots
between tool calls. Tool start/completion events share `call_id`, with their
args/result nested under a tool-call variant. Terminal `result.result` contains
the authoritative aggregate text. Official docs state that failures exit
nonzero and can end without a terminal event, writing an error to stderr.
The adapter requires an explicit successful terminal result and preserves
native process failure handling. Thinking is suppressed by native print mode.

`--force` is deliberately absent. The headless guide says file modifications
are proposed rather than applied without that flag. This is **not a read-only
filesystem or sandbox guarantee**: print mode still exposes tools and obeys
native settings/permission rules. The adapter does not add `--trust`,
`--approve-mcps`, force permissions, or sandbox changes.

Cursor's `--list-models`/`models` options are documented, but no stable
machine-readable model schema was verified. The template therefore advertises
manual model selection only. Interactive `ls`/resume commands do not prove a
read-only exact-ID/cwd metadata API, so native sessions are not advertised.
Cursor Cloud Agents and the editor UI are separate surfaces from this local
headless adapter.

## Bounds and verification

All events produce public protocol version 1 records. Completed message
snapshots have no fabricated start time. Tool identities and terminal states
are latched against duplicate events. Retained pending inputs are capped at
8 MiB; tool/message identity sets are capped at 10,000 IDs, diagnostics at 128.
Oversized individual/combined records use an explicit omission placeholder.
Only the final result record is emitted from `finish()`, once by its caller.

Both templates currently pass the prompt as an argument, so operating-system
process argument limits apply, including Windows. No native model call was
needed to validate the contracts; dedicated fixture tests cover identities,
parents, thinking omission, recovered tool errors, final errors, missing
results, duplicates, partial flush omission, malformed results, and size bounds.
