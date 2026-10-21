# Kimi Code live Server API provenance

The adapter targets native TypeScript Kimi Code **2.x**, verified with installed **2.1.1**. The retired Python kimi-cli is a different protocol. Print-mode JSONL is buffered at message boundaries and is no longer an execution entry point.

Official source snapshot: `ef727e517923cb312cb48d42825c49dfacd528e7` in [MoonshotAI/kimi-code](https://github.com/MoonshotAI/kimi-code).

- `packages/kap-server/src/routes/sessions.ts`: create with custom title and native persistence.
- `packages/kap-server/src/routes/sessionAgentConfig.ts`: cold resume activation before subscription via an empty session-level profile patch (no fields changed and no transcript download).
- `packages/kap-server/src/routes/prompts.ts`: submit once, configured model and session permission mode.
- `packages/kap-server/src/transport/ws/v1/wsConnectionV1.ts`: accepted subscriptions and application ping/pong.
- `packages/kap-server/src/protocol/events-zod.ts`: public deltas, tool lifecycle, actual terminal reason.
- `packages/kap-server/src/instanceRegistry.ts` and `services/auth/persistentToken.ts`: own-process loopback discovery and private authentication. Registry IDs and API server IDs are distinct; ownership is checked using the actual child PID.
- `packages/agent-core-v2/src/session/sessionMetadata/sessionMetadataService.ts`: custom titles and native state.

`live-server.cjs` is a synthetic credential-free protocol server, not an inference recording. Tests prove events arrive before the delayed tool completes; verify literal Chinese titles, heartbeat handling, failed/disconnected/rejected runs, directory/title mismatches, native shutdown errors, no duplicate prompt submission, and no bearer banner leakage. Oversized UTF-8 tool input/output is sent in real 64-bit-length WebSocket frames; bounded previews must preserve native completion and the complete final report, while true failed tools/turns stay failed. Oversized reports carry the existing truncation marker. Unit tests also exercise active-input retention, node-count saturation, dedup-window rollover and duplicate events. Session-index tests remain read-only and scoped to the exact canonical directory.

Real inference on 2026-10-16 verified live message/tool events, separate final report, persisted custom title, listing, exact session binding, context recall, and unchanged title after continuation. Test workspaces and raw transcripts remain ignored locally; no user conversation content or model credentials are checked in.

The helper reads the native server's redacted `/config` only to resolve a new session's configured default model. Bound sessions retain their saved model unless explicitly overridden. Authentication comes from the native local server credential, not a copied provider secret; the credential and ready banner never enter Wakeit stdout/stderr. The per-execution server is stopped after completion and its children remain under the Runner process group/job.
