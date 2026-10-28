# RFC 3: Independent CLI and Gateway releases

Status: accepted. Tracking issue: [#162](https://github.com/mob999/wakeit/issues/162).

## Product boundary

| Product | First split version | Program | Owns |
| --- | --- | --- | --- |
| Gateway | 0.4.0 | `wakeit-gateway` | WebUI, API, scheduling, storage, server administration and deployment |
| CLI | 0.1.0 | `wakeit` | Connections, remote Wakeit operations, Runner, Agent adapters, local sessions and client updates |

Runner uses the CLI version. The existing 0.1.0–0.3.4 history stays with Gateway. The Wakeit Skill remains one skill and ships with the CLI; it loads connection or deployment guidance as needed.

`wakeit` has no Gateway deployment subcommand. Server administration is available directly through `wakeit-gateway start`, `migrate`, `bootstrap`, `grant-instance-admin`, `nats-init` and `service`. Remote workspace operations remain CLI functions.

## Source boundary

Keep one repository and three Rust packages:

- `wakeit-core`: shared configuration, protocol and task types; an internal dependency, not a separate user release.
- `wakeit-cli`: the `wakeit` program and Runner. Building it requires Rust, not Node or WebUI assets.
- `wakeit-gateway`: the server program and embedded WebUI. Its source build includes the frontend build.

Gateway and CLI declare their own package versions. Config and execution-message formats retain version 1; package numbers are not protocol versions. Product versions are not compared for equality. No generic compatibility negotiation framework is introduced in this split.

## Releases and installation

| Product | Tag | Manifest | Changelog |
| --- | --- | --- | --- |
| Gateway | `gateway-vX.Y.Z` | `wakeit-gateway-release.json` | `CHANGELOG.md`, `CHANGELOG.zh-CN.md` |
| CLI | `cli-vX.Y.Z` | `wakeit-cli-release.json` | `CHANGELOG.cli.md`, `CHANGELOG.cli.zh-CN.md` |

Both products keep native packages for their supported platforms. CLI update discovery selects the `cli-v*` tag prefix, never the repository-wide latest release. Checksums and manifests are scoped to the selected product. `wakeit update` only updates the CLI.

`install.sh` installs CLI/Runner and optionally the Wakeit Skill. The script defaults to CLI 0.1.0; `--version` selects an explicit CLI version. `deploy-gateway.sh` defaults to Gateway 0.4.0 and accepts an explicit Gateway version; an existing config can be supplied to run database updates or start the server. Each product release updates only its own installer default. The scripts do not use a `latest` alias. PostgreSQL/NATS setup and service management remain in the established Compose and native deployment paths rather than being reimplemented in a second full-stack installer.

Agent Prompt uses the CLI installation script without deriving a client version from Gateway metadata. The WebUI version and release notes describe Gateway.

## Breaking transition

This is an explicit breaking change, without legacy command aliases, old-tag update routing or automatic conversion of service launch commands. The original `v*` releases remain unchanged. Users install the new CLI release explicitly and change Gateway service commands to `wakeit-gateway` before replacing a shared old executable.

Keep existing databases, NATS identities, task history, Runner registration and Agent credentials. Separating executables does not call for replacing those resources. Historical upgrade guides retain the commands that applied to their releases.

## Delivery scope

1. Split the Rust packages and command ownership; remove Gateway code and WebUI build requirements from CLI builds.
2. Split tags, package metadata, installation scripts, update discovery and release notes.
3. Update setup prompts, the Skill, source/Compose/native deployment instructions and current version labels.
4. Route CI by changed product: frontend/Gateway changes do not start CLI checks, CLI changes do not build WebUI, and shared changes select both. Keep end-to-end checks for the affected shared boundary.

Completion means each product builds and publishes independently, each installer and updater selects the intended product, the CLI has no deployment commands, and Gateway remains deployable through its documented server paths. CI owns product builds and relevant behavior checks; local work focuses on the changed setup and deployment flows.
