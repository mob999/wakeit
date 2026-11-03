# Plugin SDK

[简体中文](plugin-sdk-zh.md)

Extensions add event handlers, lifecycle hooks, actions and query providers to a plugin package. They run as supervised processes on a Runner. They do not run inside Gateway, and they do not need to provide an Agent adapter.

Package installation and source identity follow [Agent plugins](plugins.md). A package may contain `agents`, executable `commands`, `extensions`, or a combination. Each extension instance has its own settings, credentials and storage on its Runner. Installed code can be shared; instance data is not shared between workspaces.

## Describe an extension

Declare the entry point, static configuration schema, credential slots, permissions and callback IDs in `wakeit-plugin.json`:

```json
{
  "manifest_version": 1,
  "name": "example-extension",
  "version": "1.0.0",
  "wakeit": {
    "protocol": 1,
    "runtimes": ["node"],
    "extensions": {
      "example": {
        "name": "Example",
        "command": "node",
        "args": ["{{plugin.root}}/extension.js"],
        "config": {
          "type": "object",
          "properties": {"prefix": {"type": "string"}},
          "required": ["prefix"]
        },
        "credentials": {"account": {"label": "Account credential"}},
        "permissions": ["tasks:read", "hooks:execution.prepare"],
        "handlers": {"completed": {"events": ["run.finished"]}},
        "hooks": {"prepare": {"stage": "execution.prepare"}},
        "actions": {"inspect": {"label": "Inspect configuration", "input": {"type": "object"}}}
      }
    }
  }
}
```

Configuration schemas use JSON Schema. They are validated on the device; schema validation does not fetch remote references. Runtime callback registrations must match the manifest. A declared capability is not a grant: the administrator grants permissions and binds events/hooks to an instance.

Use `wakeit schema plugin-sdk` for shared data contracts and the event/hook catalog. Agent YAML and Agent output events remain version 1; extension IPC negotiates its own protocol version.

## TypeScript

The SDK source is in [`sdk/typescript`](../sdk/typescript). Bundle it into the executable resource shipped with your plugin; Wakeit does not run npm install or build scripts during installation. Node 22.18+ can execute erasable TypeScript; a compiled JavaScript bundle works too.

```ts
import { Plugin, patch } from '@wakeit/plugin-sdk'

const plugin = new Plugin()

plugin.on('completed', async (event, ctx) => {
  await ctx.storage.set('last-event', event)
})

plugin.hook('prepare', async (input, ctx) => {
  const settings = await ctx.config.get<{prefix: string}>()
  return patch({prompt: `${settings.value.prefix}\n${input.prompt}`})
})

plugin.action('inspect', async (_, ctx) => {
  return (await ctx.config.get()).value
})

await plugin.serve()
```

The callback IDs (`completed`, `prepare`, `inspect`) refer to the manifest declarations. An extension can provide several handlers for different events. Register providers with `plugin.provider`, and fixed lifecycle callbacks with `plugin.lifecycleHandler`.

## Rust and other languages

[`wakeit-plugin-sdk`](../crates/wakeit-plugin-sdk) implements the same process protocol for native extensions. `Plugin` registers callbacks; `Context` provides the invocation, cancellation receiver and typed `call` responses. Native binaries use the existing platform artifact distribution.

Other languages can implement newline-delimited JSON-RPC 2.0 over stdin/stdout. The host sends `initialize`, lifecycle requests and `invoke`. The extension can send host API requests while a callback runs. Responses may arrive out of order and are correlated by request ID. Standard output belongs to the protocol; use the structured logger for diagnostics.

## Events and hooks

Events describe committed facts. Gateway owns Task/Run/DAG events; Runner owns local execution events. `run.finished` includes ordinary runs and complete DAG rounds, not each child attempt. A queued run can finish without starting. `run.started` marks the core execution/preparation window; `execution.started` confirms the native process has actually launched. `unknown` is not finished. Reopening a DAG and completing it again produces a new completion event.

A delivery preserves its event ID across retries. Explicit replay creates a new delivery and idempotency key. An external side effect may have completed before a process disconnects: handlers must use an external idempotency key where supported and report uncertain outcomes accurately.

Completion events reference immutable snapshots. The event payload sent to an extension contains small metadata and a snapshot reference; read the saved result through the host API rather than loading the current Run, which may have been retried. Delivery expiry and content retention are explicit.

Hooks run before an operation commits. They return `allow`, `deny` or `patch`. Each stage has an allowlist of mutable fields. Execution preparation can modify the prompt and declared parameters; it cannot replace the Runner, Agent, working directory, Session or Run identity. Core validation runs after transformations.

Hooks execute in configured order. Gateway hooks run on the selected device without holding database row locks while waiting. Configured failure policy determines whether an unavailable handler rejects, waits or is explicitly skipped; a business `deny` is not silently skipped. Hook context only permits read APIs. Put external effects in committed-event handlers or actions.

## Device configuration

Open **Devices → device → Plugins** to create and configure instances, set credentials, bind handlers and inspect activity. The same management contract is available through CLI:

```sh
wakeit --json plugin instance --runner RUNNER_ID catalog
wakeit --json plugin instance --runner RUNNER_ID create --file instance.json
wakeit --json plugin instance --runner RUNNER_ID configure --file settings.json
```

`create` input contains `source`, `extension`, `name`, `config` and granted `permissions` as `grants`. To make creation retryable, supply `instance_id`. Configuration input contains `instance_id` and:

```json
{
  "update": {
    "operation_id": "GENERATED_UUID",
    "expected_revision": 1,
    "config": {"prefix": "Example"},
    "secrets": {"account": {"operation": "replace", "value": "LOCAL_SECRET"}}
  }
}
```

Secret updates distinguish `keep`, `replace` and `clear`. Readback only reports whether a slot is configured. Secret values are transmitted to Runner through live authenticated requests; they are not saved in Gateway operations or the event queue. Extension code is trusted local code, not an operating-system sandbox.

Code versions and ordinary configuration revisions are recorded for invocations. Changes do not overwrite the context of an already queued delivery. Updates retain versioned resources; an upgrade validates configuration and capabilities before switching instances. A failed candidate does not replace a working version. Updating package resources and activating an instance are separate operations.

Remote package installation/update/removal requires the device owner's explicit policy. Set `{"remote_package_management":true}` in `DATA_DIR/plugins-policy.json` to allow workspace administrators to request those operations. Configuration and instance management do not require granting arbitrary remote shell access.

## Host APIs and interactions

Host calls use named business methods rather than arbitrary authenticated HTTP. Namespace grants and current workspace membership apply. User actions also retain their initiating user's permissions; an extension never receives a Gateway or Runner token.

The SDK exposes configuration, credential slots, private versioned storage, business queries/mutations, event delivery, hooks, actions, provider calls, controlled external HTTP and managed subprocess execution. Mutations use request keys and object revisions where applicable. `http:request`, `process:execute` and `auth:manage` are separate grants.

Actions can request schema forms, choices, confirmations, secret input, authorization URLs or device codes, and report progress. WebUI renders these using shared controls. Closing a view is not an implicit cancellation; cancellation is an explicit host operation.

OAuth supports authorization code with PKCE and device authorization. Verifiers and tokens are stored on Runner. Gateway validates a temporary callback state and forwards the authorization response to that instance. Provider requirements still apply: a deployment without a reachable callback needs a provider-supported device-code or local flow. Token refresh/revocation are explicit credential operations.

## Contract checks

[`fixtures/plugins/generic`](../fixtures/plugins/generic) is a synthetic extension using the TypeScript SDK. It validates generic settings, write-only credentials, allow/deny/patch, host callbacks, durable event delivery/replay, actions, structured interaction and cancellation. It does not send messages or use real external credentials.

The complete coverage and semantics are tracked in [RFC 4](rfc-4-plugin-sdk.md).

## Pending operations and public streams

When a workspace operation has remote hooks, Gateway accepts it with HTTP 202 and an operation receipt. The CLI and WebUI follow that receipt to its committed result; SDK callers can query `runtime.operation` or cancel with `runtime.cancel`. A cancelled proposal cannot later commit. Authority and Hook configuration revisions are checked again in the committing transaction.

Use `ctx.streams.events(runId, {after, kind, signal})` in TypeScript, or `api::EventCursor` in Rust, for cursor-based public progress. These read the existing bounded execution recording rather than enqueueing a normal durable callback for every token. Truncation/expiry markers remain visible. Completion handlers continue to use durable event delivery.

`files.stat/read` provide revision-checked chunks. `files.watch/unwatch` subscribe to an explicitly permitted directory and report gaps if the watcher falls behind. `artifacts.register/export` creates an authorized reference; download interactions refer to that artifact ID. They cannot turn another device's path into a local file.
