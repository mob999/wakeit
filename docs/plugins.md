# Agent plugins

[简体中文](plugins-zh.md)

The CLI includes an official Agent plugin. The SDK integration below describes the current source contract, not a claim that every released CLI supports it. When targeting an installed release, use that release's schema and advertised capabilities.

Installation is unchanged. Install a third-party plugin on the device where its Runner executes:

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
    "runtimes": ["node"],
    "plugins": {
      "example": {
        "name": "Example plugin",
        "command": "node",
        "args": ["{{plugin.root}}/plugin.js"],
        "agents": {
          "example": {
            "name": "Example Agent",
            "mode": "sync",
            "parameters": {"model": {"type": "string", "label": "Model"}},
            "operations": ["detect", "execute"]
          }
        }
      }
    }
  }
}
```

`wakeit.plugins` declares one entry point per plugin, which can provide Agents, event handlers, hooks, actions and providers together. Keys in its `agents` map define Agent IDs; each Descriptor declares the name, optional logo, parameters and supported operations. The program registers callbacks through SDK `Plugin.agent(id, Agent)`; initialization checks registrations against the declaration. An Agent is not an ordinary action. See the [Plugin SDK](plugin-sdk.md) for implementation.

`command: ./bin/example` resolves from the package root. Use `{{plugin.root}}/plugin.js` for package files in arguments. The Agent request carries the task working directory; bare commands such as `node` use PATH. Declare required interpreters in `runtimes`; Wakeit does not run build hooks or install dependencies.

### Legacy compatibility

Existing standalone user Agent YAML and `wakeit.agents: {"example": "agents/example.yaml"}` continue to use the [command adapter protocol](protocol.md). The manifest defines the Agent ID; legacy package YAML can omit `id`. The old `wakeit.extensions` field remains a compatibility input for plugin declarations; use `wakeit.plugins` for new packages. Do not declare the same plugin or Agent through both forms.

The official package uses static Descriptors under `wakeit.plugins.official` and starts its unified entry with `args: ["sdk"]`. It does not generate or read official Agent YAML. Older CLIs may reject a new manifest; do not depend on silently ignored fields or an ordinary-action fallback.

Native plugins can supply a gzip tar archive for each platform under `wakeit.artifacts`, or use `wakeit.artifact` for a common URL template:

```json
{
  "aarch64-apple-darwin": {
    "url": "https://example.com/releases/v1.0.0/macos-arm64.tar.gz",
    "sha256": "<64 hexadecimal characters>"
  }
}
```

Archive paths are relative to the package root and contain regular files/directories. Platform archives supply executable resources; they cannot replace the manifest or legacy Agent YAML checked out from Git. Alternatively, `checksums_url` may point to a SHA256SUMS file containing the artifact filename. An unsupported platform or missing runtime produces an installation error. URLs can use `{version}` from the package manifest and `{target}` from the device platform. Check package integrity with `wakeit plugin validate --dir ./my-agent`. See `wakeit schema plugin` for the complete manifest schema.

Optionally declare `wakeit.commands`, a map of command names to package-relative executable paths. Invoke one without changing cwd using `wakeit plugin exec SOURCE COMMAND -- ARGS`. The official package exposes `adapter` for direct detection or session queries, for example:

```sh
wakeit plugin exec git:github.com/mob999/wakeit adapter -- detect kimi-code
```

## Upgrade from CLI 0.1

Install the complete CLI bundle using the [installer](install.md). The old `wakeit adapter` commands have been removed. To use official SDK Agents, set `bundled_agents: true` and remove the corresponding old official definitions from the Runner's `agents` path list to avoid duplicate IDs. Keep standalone custom YAML where needed; `agents` can be empty when only the official package is used.

SDK Agents do not need YAML generated by `wakeit agent init`; that command applies only to compatible adapters still declared with YAML. Preserve required custom configuration, device identity and credentials.

New setup/enrollment uses plugin resources directly. Keep device credentials and Runner identity; no Gateway upgrade is required. New plugin-backed sessions retain the executable path from their package version.

Official adapters use SDK Agent callbacks for new and resumed executions and construct separate native argv values. The standalone `adapter -- run …` helper remains for protocol diagnostics, not official registration. Kimi runs its native ACP over stdio. Existing native session titles are preserved; when a verified session has none, Runner keeps a local display name used in session inspection and search. Later native titles take precedence. Third-party command and event interfaces are unchanged.

## General extensions

Packages can also provide event handlers, lifecycle hooks and device-configured actions. See the [Plugin SDK](plugin-sdk.md).
