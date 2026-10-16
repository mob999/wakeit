# Antigravity CLI fixtures

These are synthetic protocol fixtures, checked against primary upstream sources on 2026-10-16. They are not captured inference sessions. No Antigravity binary, authentication state, or user conversation is included.

- `agy-stream.ndjson` combines the documented `init`, incremental `agent_response`, `run_command`, checkpoint, and terminal-result shapes from [Headless mode](https://antigravity.google/docs/cli/headless/). IDs and response text are test data.
- `agy-models.json` uses the actual envelope reproduced in [upstream issue #777](https://github.com/google-antigravity/antigravity-cli/issues/777): `command.data.models` entries have `id` and `label`. The maintainer confirms that global output flags precede the subcommand: `agy --output-format json models`.
- The official [changelog](https://github.com/google-antigravity/antigravity-cli/blob/main/CHANGELOG.md) documents `--disable-slash-commands` in 1.1.9 and streaming stdin in 1.1.15.

The adapter preserves native permission policy. Headless approval requests are soft-denied, with notices on stderr; native tool errors can be recovered by the agent. Only a successful terminal result completes the Run. The config does not enable `--dangerously-skip-permissions`.

Native `--conversation <id>` resume is documented. A supported machine-readable native session list/inspect interface has not been verified, so this integration does not advertise the full wakeit session contract or assume native persistence. Reasoning effort remains an optional manually entered native argument; the model catalog does not advertise unverified per-model effort choices.

`web/public/agent-logos/agy.svg` is the original vector [official site favicon](https://antigravity.google/favicon.svg), with original geometry and theme styling preserved. Antigravity CLI is downloaded and authenticated separately by the user; wakeit does not redistribute its binary or modify its credentials.
