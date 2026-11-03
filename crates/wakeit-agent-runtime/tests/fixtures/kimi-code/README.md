# Kimi ACP protocol fixtures

The adapter drives native Kimi Code through `kimi acp` over stdio. Protocol reference: [Kimi ACP](https://www.kimi.com/code/docs/en/kimi-code-cli/reference/kimi-acp.html). The retired Python kimi-cli uses a different protocol.

`acp-agent.cjs` is a synthetic, credential-free ACP peer. It covers initialize, new/resume, model and mode selection, incremental message/tool updates, normal completion, cancellation, permission requests, malformed lifecycle completion and process failure. Native index/state files are disposable metadata fixtures used to verify exact session and working-directory identity. No inference recordings or user data are included.

Protocol normalization tests keep tool output separate from the final answer, omit thought chunks and bound retained previews. Host tests verify real pipes, child ownership and final-result publication after successful shutdown. The official adapter uses the same version 1 events contract as third-party programs.
