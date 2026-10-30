# Contributing to Wakeit

Keep changes focused on a concrete problem. For installation and usage, start with the [README](README.md).

## Development setup

- **Rust 1.89+** for the CLI, Runner and Gateway.
- **Node.js 22.12+** for the WebUI and Gateway builds.
- **Python 3.12+** for documentation, packaging and integration checks.
- **PostgreSQL 16+ and NATS Server 2.15+** for running the Gateway and service integration tests.

Build the CLI, which includes the Runner:

```sh
cargo build --locked -p wakeit-cli -p wakeit-agent-runtime
python3 scripts/package-release.py stage --input target/debug/wakeit --output target/debug/wakeit-plugins/0.2.0
```

The stage command prepares the same complete plugin bundle used by releases. On Windows, use `target/debug/wakeit.exe` as the input. `WAKEIT_BUNDLED_PLUGIN_DIR` can select another prepared bundle.

Build the Gateway with its embedded WebUI:

```sh
npm --prefix web ci
npm --prefix web run build
cargo build --locked -p wakeit-gateway
```

See the [Gateway guide](docs/gateway-service.md) for configuration and startup. Source lives in `crates/wakeit-cli`, `crates/wakeit-gateway`, `crates/wakeit-core`, `crates/wakeit-agent-runtime` and `web/src`.

## WebUI hot reload

Use one Vite server serving the working tree directly. With a Gateway listening on `127.0.0.1:8080`, set its `public_url` to `http://127.0.0.1:5173`, then run:

```sh
cd web
WAKEIT_DEV_GATEWAY=http://127.0.0.1:8080 \
WAKEIT_DEV_PUBLIC_URL=http://127.0.0.1:5173 \
  npm run dev -- --port 5173 --strictPort
```

Use the Vite address in your browser and Runner connection. Vite proxies API requests, WebSockets and live logs to the Gateway. React and CSS changes reload automatically; frontend edits do not require rebuilding or restarting the backend.

Follow the [UI design guide](docs/design.md). Add interface text to both `web/src/locales/en.json` and `zh.json`; leave task instructions, reports, logs and identifiers unchanged.

## Verification

Run focused checks when they help verify the changed behavior. CI selects checks by scope and handles full suites, platform builds, service integration and packaging. There is no need to repeat that pipeline locally.

For example, run CLI tests with `cargo test --locked -p wakeit-cli`, or frontend unit tests with `npm --prefix web run test:unit`. See the [test strategy](docs/test-strategy.md) for coverage and integration workflows.

Keep tests that protect behavior, data boundaries and real integrations. Inspect UI changes through the dev preview using synthetic data. Screenshots and temporary comparison scripts stay local under `.local/`; never upload screenshots containing real user data or credentials.

## Submit a pull request

1. Open or link an issue, then branch from the latest `main`. Use a type and scope prefix, such as `feat(gateway)/device-auth` or `fix(runner)/reconnect`.
2. Make the change and update relevant documentation. Keep the English and Chinese READMEs and changelogs aligned. Add database migrations rather than editing published ones.
3. Open a PR linked to the issue. Explain the problem, resulting behavior and relevant verification. Push follow-up commits to address review feedback.
4. Merge after the current commit's required CI passes and discussions are resolved, then delete the remote branch.

Do not push directly to `main`, force-push or rewrite published commits and tags. Keep runtime data, local configuration and credentials out of Git. See [AGENTS.md](AGENTS.md) for repository conventions.

## Releases

Gateway/WebUI and CLI/Runner are versioned independently; the Wakeit Skill ships with the CLI. Release tags use `gateway-vX.Y.Z` and `cli-vX.Y.Z`.

Prepare a release through a PR, aligning the affected product's version, installer default and bilingual changelog. Notes describe user-visible changes since the previous release. The [release workflow](.github/workflows/release.yml) builds artifacts on manual runs and publishes official releases from matching tags.
