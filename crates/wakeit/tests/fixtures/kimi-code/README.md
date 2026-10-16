# Kimi Code fixture provenance

`stream.jsonl` is a synthetic, credential-free fixture based on the current
TypeScript Kimi Code CLI's `PromptJsonWriter` and `writeResumeHint`. It is not an
inference capture. Verified on 2026-10-16 against official source commit
`ef727e517923cb312cb48d42825c49dfacd528e7` and the installed native ARM64
`~/.kimi-code/bin/kimi` version `2.1.1` (`--version` / `--help` only).

Sources:

- [Current CLI reference](https://www.kimi.com/code/docs/en/kimi-code-cli/reference/kimi-command.html)
- [Prompt JSON writer](https://github.com/MoonshotAI/kimi-code/blob/ef727e517923cb312cb48d42825c49dfacd528e7/apps/kimi-code/src/cli/prompt-render.ts)
- [Prompt lifecycle and native resume](https://github.com/MoonshotAI/kimi-code/blob/ef727e517923cb312cb48d42825c49dfacd528e7/apps/kimi-code/src/cli/v2/run-v2-print.ts)
- [Official session list CLI](https://github.com/MoonshotAI/kimi-code/blob/ef727e517923cb312cb48d42825c49dfacd528e7/apps/kimi-code/src/cli/sub/session.ts)
- [SDK session summary fields](https://github.com/MoonshotAI/kimi-code/blob/ef727e517923cb312cb48d42825c49dfacd528e7/packages/node-sdk/src/types.ts)
- [Native metadata persistence and resume](https://github.com/MoonshotAI/kimi-code/blob/ef727e517923cb312cb48d42825c49dfacd528e7/packages/agent-core-v2/src/workspace/sessionLifecycle/sessionLifecycleService.ts)
- [Session state metadata](https://github.com/MoonshotAI/kimi-code/blob/ef727e517923cb312cb48d42825c49dfacd528e7/packages/agent-core-v2/src/session/sessionMetadata/sessionMetadataService.ts)
- [Native data-directory resolution](https://github.com/MoonshotAI/kimi-code/blob/ef727e517923cb312cb48d42825c49dfacd528e7/apps/kimi-code/src/utils/paths.ts)

Session discovery reads `<KIMI_CODE_HOME>/session_index.jsonl` (otherwise
`~/.kimi-code/session_index.jsonl`) and only opens `sessionDir/state.json` for
the matching canonical project directory. The current native create and fork
paths append this index. The newest mapping wins; persisted metadata must
match both its complete ID and project directory. Missing or mismatched state
is excluded. Session titles come only from `title`, never `lastPrompt`.
Index reading is streamed with a 1 MiB line limit, 64 MiB total limit, and
100,000-entry limit; state metadata is limited to 1 MiB. The adapter never
starts `kimi session list` or boots the SDK for discovery, avoiding runtime
initialization and provider refresh side effects.

The retired Python `MoonshotAI/kimi-cli` is a different CLI/protocol. This
adapter uses `--prompt TEXT`, not its `--print`/Wire interfaces. Current prompt
mode already uses automatic permissions and rejects `--auto`/`--yolo` alongside
`--prompt`.

Prompt input is a single command argument: the current CLI has no documented
stdin prompt interface. Long prompts therefore remain subject to operating
system command-line limits, including Windows process argument limits.

No model discovery is advertised. `kimi provider list --json` exposes raw
provider configuration and calls `ensureConfigFile()`, so it cannot guarantee
read-only discovery. The model alias can be supplied explicitly with `--model`.
Print JSON does not expose native tool `isError`, exit codes, real tool start
times, or thinking content; the adapter does not infer these from tool text.
No official vector logo was verified, so no logo asset is supplied.
