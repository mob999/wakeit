# GitHub Copilot CLI JSONL contract

Checked 2026-10-16 against native Copilot CLI 1.0.98, its `--help`, [programmatic reference](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-programmatic-reference), and real ordinary-prompt output using an explicitly configured OpenAI-compatible provider.

The template uses piped stdin and `--no-ask-user --output-format json`; it omits `--silent`, which suppresses progress. It does not use the SDK transport or the separate workflow schema. `stream.jsonl` is synthetic and contains no captured conversations, credentials, or machine paths.

Native records verified in a real invocation: `assistant.message_start`, `assistant.message_delta` (`deltaContent`), `assistant.message` (`content` and `toolRequests`), `tool.execution_start` (`toolCallId`, `toolName`, `arguments`), `tool.execution_complete` (`success`, `result` or `error`), and a top-level `result` (`exitCode`, not `data.exitCode`). Public content becomes complete Trace snapshots. Hidden reasoning fields are ignored. The terminal result must have exitCode 0, all steps must be closed, and the child must exit successfully before one final report is emitted.

Native tool permissions remain unchanged. A denied tool is shown as failed; a later successful Agent answer does not invent tool success. The real audit tested denial and a separately pre-approved harmless command. No allow-all permissions are added to the production template. Session metadata in an event is not treated as proof of persistence; this adapter does not advertise Wakeit session binding or model discovery.

Process regression coverage verifies literal multiline stdin/model argv, fragmentation, nonzero exit, malformed events, early Trace delivery, tool failures, missing terminals, duplicate records, and report separation. See [the full audit](../../../../../docs/agent-verification.md) for real execution coverage and limits.
