## 0.2.0

- Install and update third-party Agent plugins from Git repositories or local directories with `wakeit plugin`.
- Official Agent adapters come preinstalled and use the same plugin format as third-party adapters.
- CLI installation and updates include the official plugin bundle. Existing tasks keep their Agent IDs.
- Large output no longer blocks results from other runs or leaves completed tasks occupying execution slots while results wait to upload.
- **Breaking:** removed `wakeit adapter` commands. Regenerate official Agent YAML with `wakeit agent init`; new Runner setup uses plugin resources directly. Gateway does not require an upgrade.

## 0.1.1

- Install or update the Wakeit Skill with `wakeit skill install`, with a default location and an optional custom directory.
- Simplified installation and usage documentation.

## 0.1.0

- Connect to a Wakeit workspace through browser authorization and manage saved connections.
- Register devices and run Agents through the bundled Runner, with user-level autostart and existing Agent credentials.
- Manage tasks, presets, runs, devices and members from the command line.
- Follow live execution, read results and continue supported Agent sessions.
- Install and update the CLI independently of Gateway. The Wakeit Skill is included in CLI releases.

The CLI does not deploy Gateway. Use the separate [Gateway installation](https://github.com/mob999/wakeit/blob/cli-v0.1.0/docs/gateway-service.md) for hosting a service.
