# Agent plugins

[简体中文](plugins-zh.md)

CLI 0.2 includes an official plugin with all supported Agent adapters. Install a third-party plugin on the device where its Runner executes:

```sh
wakeit plugin install https://github.com/example/wakeit-agent
wakeit plugin install git:github.com/example/wakeit-agent@v1.0.0
wakeit plugin install ./my-agent
wakeit plugin list
wakeit plugin update
wakeit plugin remove https://github.com/example/wakeit-agent
```

Install, update and remove take effect when the Runner next starts. They do not restart services or interrupt running tasks. `list` shows installed packages and local Runner loading state. Add `--json` for automation.

## Sources and versions

Git packages are identified by repository, ignoring the ref and transport spelling. A tag or commit stays pinned. An unpinned source follows its default branch; an explicit branch follows that branch, only when you run `update`. Reinstall the same source with another ref to change the selected version.

Local packages use their absolute directory and load directly from it. Removing one does not delete the author's files, even if the directory has already been moved or deleted.

A package's `name` is display information. Tasks still select an Agent ID such as `kimi-code`. If packages provide the same Agent, select its source explicitly:

```sh
wakeit plugin install https://github.com/example/custom-kimi --replace-agent kimi-code
```

This selects that source for the Agent on this device. Existing standalone Agent YAML remains supported; duplicate IDs are reported rather than resolved by loading order.

The official package ships with the CLI. CLI updates refresh its bundled version while preserving an explicitly installed version or removal choice. Package files from previous versions remain available for running processes and saved sessions. Gateway does not install or execute plugins.

## Create a plugin

Place `wakeit-plugin.json` at the repository root:

```json
{
  "manifest_version": 1,
  "name": "example-agent",
  "version": "1.0.0",
  "wakeit": {
    "protocol": 1,
    "agents": {"example": "agents/example.yaml"},
    "runtimes": ["node"]
  }
}
```

The manifest keys define Agent IDs; package YAML can omit `id`. An Agent YAML uses the existing [adapter protocol](protocol.md). `command: ./bin/example` resolves from the package root. Use `{{plugin.root}}/adapter.cjs` for package files in arguments; the process working directory remains the task's directory. Bare commands such as `node` use PATH. Declare any required interpreters in `runtimes`; Wakeit does not run build hooks or install dependencies.

Native plugins can supply a gzip tar archive for each platform under `wakeit.artifacts`, or use `wakeit.artifact` for a common URL template:

```json
{
  "aarch64-apple-darwin": {
    "url": "https://example.com/releases/v1.0.0/macos-arm64.tar.gz",
    "sha256": "<64 hexadecimal characters>"
  }
}
```

Archive paths are relative to the package root and contain regular files/directories. Platform archives supply executable resources; they cannot replace the manifest or Agent YAML checked out from Git. Alternatively, `checksums_url` may point to a SHA256SUMS file containing the artifact filename. An unsupported platform or missing runtime produces an installation error. URLs can use `{version}` from the package manifest and `{target}` from the device platform. Check package integrity with `wakeit plugin validate --dir ./my-agent`. See `wakeit schema plugin` for the complete manifest schema.

Optionally declare `wakeit.commands`, a map of command names to package-relative executable paths. Invoke one without changing cwd using `wakeit plugin exec SOURCE COMMAND -- ARGS`. The official package exposes `adapter` for direct detection or session queries, for example:

```sh
wakeit plugin exec git:github.com/mob999/wakeit adapter -- detect kimi-code
```

## Upgrade from CLI 0.1

Install the complete CLI 0.2 bundle using the [installer](install.md). The old `wakeit adapter` commands have been removed. If a Runner explicitly lists old official YAML files, regenerate them with `wakeit agent init ID --file NEW_FILE` and update its `agents` paths, or use `bundled_agents: true` with an empty `agents` list. Preserve any custom Agent options you need.

New setup/enrollment uses plugin resources directly. Keep device credentials and Runner identity; no Gateway upgrade is required. New plugin-backed sessions retain the executable path from their package version.
