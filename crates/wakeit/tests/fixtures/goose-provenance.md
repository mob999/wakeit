# Goose protocol evidence

Verified 2026-10-16 against AAIF/Block Goose's official stable
[v1.54.0 release](https://github.com/aaif-goose/goose/releases/tag/v1.54.0).
This is `aaif-goose/goose`, formerly `block/goose`, rather than the unrelated
Goose Code product. The fixture contains synthetic data only.

- [CLI command reference](https://github.com/aaif-goose/goose/blob/v1.54.0/documentation/docs/guides/goose-cli-commands.md)
  and [CLI arguments](https://github.com/aaif-goose/goose/blob/v1.54.0/crates/goose-cli/src/cli.rs)
  establish `goose run --instructions - --output-format stream-json --quiet`
  as the headless stdin command. `--provider` and `--model` are native overrides.
- [StreamEvent and producer](https://github.com/aaif-goose/goose/blob/v1.54.0/crates/goose-cli/src/session/mod.rs)
  define JSONL `message`, `notification`, `error`, and `complete` events.
  `complete` is emitted only when the headless run has no terminal error.
  Unhandled errors, cancellation, and a final message error propagate as errors.
- [Message definitions](https://github.com/aaif-goose/goose/blob/v1.54.0/crates/goose-provider-types/src/conversation/message.rs)
  define camelCase `toolRequest`, `toolResponse`, `toolCall`, `toolResult`,
  `userVisible`, and `outputTokenLimitReached`. Tool call/result envelopes
  discriminate `status: success`/`status: error`; an MCP result also has `isError`.
- [Conversation accumulation](https://github.com/aaif-goose/goose/blob/v1.54.0/crates/goose-provider-types/src/conversation.rs)
  and [CLI streaming renderer](https://github.com/aaif-goose/goose/blob/v1.54.0/crates/goose-cli/src/session/output.rs)
  append text chunks rather than treating them as cumulative snapshots.
  The adapter joins the final assistant turn and excludes earlier tool commentary.

The installed Goose mode is preserved. In headless mode, Approve/SmartApprove
reject confirmation requests instead of waiting for an interactive prompt.
Other modes retain native behavior, and elicitation without a terminal errors.
The template does not inject `GOOSE_MODE=auto` or bypass configured permissions.
The native keyring may still require the user's OS authorization; this adapter
does not change credential storage or read credentials.

No model catalog is advertised: `local-models list` only lists local inference
weights and is not the configured provider's available-model catalog. The
native `--model` and `--provider` overrides remain available. Session support is
not advertised until a separate adapter verifies full IDs, exact cwd, persistence,
and read-only inspect/list semantics. StreamEvent itself contains no session ID.

`web/public/agent-logos/goose.svg` is the original vector mark from the official
[documentation asset](https://github.com/aaif-goose/goose/blob/v1.54.0/documentation/static/img/goose.svg).
Its paths and viewBox are unchanged. A dark-theme media rule adapts the monochrome fill; it follows the page color scheme.
