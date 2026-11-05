# Plugin SDK

[简体中文](plugin-sdk-zh.md)

This guide describes the SDK contract in the current source tree, not a claim that every released version implements these capabilities. For an installed CLI, use its schema and supported operations.

The unified SDK registers Agents, event handlers, lifecycle hooks, actions and query providers in a plugin package. Runner supervises their processes; they do not run inside Gateway. A general extension does not need to provide an Agent.

Package installation and source identity follow [Agent plugins](plugins.md). New packages declare unified entry points in `wakeit.plugins`; executable `commands` and legacy `agents`/`extensions` inputs remain compatible. Each extension instance has its own settings, credentials and storage on its Runner. Installed code can be shared; instance data is not shared between workspaces.

## Agent callbacks

Declare Agent Descriptors under the plugin's `agents` map, as shown in [Agent plugins](plugins.md#create-a-plugin). A synchronous Agent declares `detect` and `execute`; an asynchronous Agent declares `detect`, `submit` and `poll`. Declare optional reconciliation, models and session operations only when implemented. A separate `cancel` operation applies to native jobs, not to cancellation of an active synchronous request. Session operations require inspection. Callbacks must match the declared operations, rather than pretending an ordinary action is an Agent.

The TypeScript registration shape below assumes your `adapter.js` supplies the manifest Descriptor and typed detection/execution callbacks:

```ts
import { Agent, Plugin } from '@wakeit/plugin-sdk'
import { descriptor, detect, execute } from './adapter.js'

await new Plugin()
  .agent('example', new Agent(descriptor)
    .on('detect', detect)
    .on('execute', execute))
  .serve()
```

Rust uses `wakeit_plugin_sdk::agent::Agent` with `Agent::new(descriptor).on(AgentOperation::Detect, detect).on(AgentOperation::Execute, execute)` and `Plugin::new().agent("example", agent)`. Both SDKs expose the Agent contract without a dependency on Core, CLI or Gateway. Rust shares wire DTOs through `wakeit-plugin-protocol`; plugin authors import the SDK, not host configuration types. Rust task APIs accept public JSON configuration (`save_task(id, config, revision, request_key)`), not Core `TaskConfig`.

The host sends typed `AgentRequest` values through `agent.invoke`; callbacks return `AgentReply`. Execution context includes the authorized working directory, deadline, parameters, Run/Task identity and task instructions. Non-execution queries do not receive task instructions. Session operations use an exact native Session ID; they must not fall back to “latest” or create a session while inspecting.

During execution, use the Agent context's `record`, `trace`, `log`, `session` and `result` helpers for public output. Each `agent.record` carries the request/Run identity and sequence and waits for host acknowledgement. The enclosed output remains a version-1 `StreamRecord`; do not write bare event JSONL or native output onto the JSON-RPC stdout channel. Keep diagnostics and private/native data out of public records.

### Native lifecycle and output completion

Report native lifecycle observations through `agent.fact` using a Run-bound `AgentFact {request_id, run_id, seq, fact: NativeFact}`. The host acknowledges facts just as it acknowledges `agent.record`. Facts and records share **one ordered sequence** for the invocation; do not maintain a separate fact counter or change the bound request/Run identity.

| Native fact | Meaning |
|---|---|
| `started {pid}` | The native process was actually created; preparation alone is not a start. |
| `input_submitted` | Task instructions were actually delivered to native execution, not merely prepared. |
| `native_finished {exit_code}` | The native process finished; this alone does not complete the authoritative Run. |
| `start_failed {message}` | Native process creation failed; include the failure reason. |

When no native work was started or submitted, return `ExecutionOutcome::NotStarted`, serialized as `not_started {message}`. A known spawn failure must not be reported as `unknown`/`uncertain`. Conversely, a missing reply is not proof that work never started; retain uncertainty when execution cannot be established.

Provide the actual final report through either `ctx.result(value)` (an acknowledged `StreamRecord::Result`) or `ExecutionOutcome::Completed.result`; duplicating it is not required. If an acknowledged stream result exists and `Completed.result` is `null`, Runner uses the stream result: this is a valid stream-only result, not an invalid placeholder. The current ABI uses a JSON `Value` field rather than an `Option` or a distinct “not supplied” state, so `null` must not be treated as universally illegal.

If both channels provide non-null reports, they must be equal, including explicit truncation markers. Conflicting reports must fail rather than silently overwrite one another. Official adapters currently return the same real report through both channels. A lifecycle fact does not substitute for the final report.

The SDK closes the invocation's output barrier on success, callback failure and timeout. Pending output must be accounted for before the invocation is finalized; late or detached callbacks cannot keep writing after the barrier closes. After cancellation, only cleanup records/facts bound to the original Run are permitted while that barrier remains open. Cancellation does not authorize another Run, new process starts, credential access or other new host work.

### Cancellation controls

For an active synchronous Run, Runner sends a `cancel` control frame to the **existing SDK request**. The adapter observes the `AgentContext` cancellation signal and stops its native work; Runner retains forced process cleanup when cooperative cancellation does not finish. This path neither starts a new worker nor invokes a separate `AgentOperation::Cancel` callback.

All official Agents currently execute synchronously and use that active-request control path. They remain cancellable (`AgentInfo.cancellable` is `true`) without advertising an independent `cancel` operation in their Descriptors.

The public `AgentOperation::Cancel` is for an asynchronous native job identified by `job_id`. Advertise it only when the callback actually requests cancellation of that job; use `poll` to confirm the stopped state. An accepted cancellation request is not confirmation that the job stopped. A worker without native-job cancellation support returns `unsupported`, rather than advertising a no-op callback that merely returns acceptance.

Runner owns execution identity, concurrency, process supervision, recording and authoritative Run state. Native completion, a cancellation request and confirmed termination are different outcomes. Report uncertain work as uncertain; reconciliation must not blindly submit it again. The official package uses this same SDK entry with `args: ["sdk"]`, without an official YAML path.

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
    "plugins": {
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

Use `wakeit schema plugin-sdk` for shared data contracts and the event/hook catalog. Legacy Agent YAML, tasks and Agent output events remain version 1; plugin IPC negotiates capabilities separately. A host that does not support a new declaration must reject it rather than silently downgrade.

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

Other languages can implement newline-delimited JSON-RPC 2.0 over stdin/stdout. The host sends `initialize`, lifecycle requests, general-extension `invoke` and typed-Agent `agent.invoke`. The extension can send host API requests while a callback runs. Responses may arrive out of order and are correlated by request ID. Standard output belongs to the protocol; use the structured logger for diagnostics.

## Events and hooks

Events describe committed facts. Gateway owns Task/Run/DAG events; Runner owns local execution events. `run.finished` includes ordinary runs and complete DAG rounds, not each child attempt. A queued run can finish without starting. `run.started` marks the core execution/preparation window; `execution.started` confirms the native process has actually launched. `unknown` is not finished. Reopening a DAG and completing it again produces a new completion event.

A delivery preserves its event ID across retries. Explicit replay creates a new delivery and idempotency key. An external side effect may have completed before a process disconnects: handlers must use an external idempotency key where supported and report uncertain outcomes accurately.

Completion events reference immutable snapshots. The event payload sent to an extension contains small metadata and a snapshot reference; read the saved result through the host API rather than loading the current Run, which may have been retried. Delivery expiry and content retention are explicit.

Hooks run before an operation commits. They return `allow`, `deny` or `patch`. Each stage has an allowlist of mutable fields. Execution preparation can modify the prompt and declared parameters; it cannot replace the Runner, Agent, working directory, Session or Run identity. Core validation runs after transformations.

Hooks execute in configured order. Gateway hooks run on the selected device without holding database row locks while waiting. Configured failure policy determines whether an unavailable handler rejects, waits or is explicitly skipped; a business `deny` is not silently skipped. Hook context only permits read APIs. Put external effects in committed-event handlers or actions.

## Device configuration

Open **Devices → device → Plugins** to configure instances, bind handlers and inspect activity. **Add plugin** collects the name, settings, credential slots and permission choices in one form. No permissions are selected by default, and enabling after setup is an explicit choice. If setup is interrupted after creation, continue with the saved instance rather than creating another one; the form retains its entries until closed. The same management contract is available through CLI:

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

### Bind a configured SDK Agent to a local instance

Runner configuration version 1 supports `agent_instances`, an optional map from Agent ID to a local plugin instance UUID; omitting it is equivalent to an empty map. Official Agents need no instance or binding by default because they have no instance-specific configuration or credentials. A configured SDK Agent must have an explicit binding; neither a matching display name nor the first item in a list selects an instance.

1. Install the package on the executing Runner and use **Devices → device → Plugins**, or the CLI commands above, to create an instance on that device. Select the installed `source` and the plugin resource key from `wakeit.plugins` (the management API calls that key `extension`). Configure its declared fields and credential slots, grant the required declared permissions, and enable the instance.
2. Obtain the instance UUID from the creation response or `plugin instance --runner RUNNER_ID catalog`. Bind it to the Agent ID in that Runner's local `runner.yaml`, not in a Task or Agent parameter.
3. Save the file and apply it through the existing Runner configuration reload procedure; current Runner configuration changes take effect after restarting Runner. Saving the file does not switch an already-started Run to another instance.

For example, for a plugin whose configuration schema declares an `endpoint` string, a CLI creation file can contain the following. Replace `SOURCE_FROM_CATALOG`, the resource key, configuration and grants with those of the installed plugin; the UUID below is synthetic:

```json
{
  "instance_id": "8c311e35-4bec-4fc4-a6ae-f3654b3ff8b4",
  "source": "SOURCE_FROM_CATALOG",
  "extension": "example",
  "name": "Work account",
  "config": {"endpoint": "https://service.example.com"},
  "grants": []
}
```

After configuring and enabling that instance, merge this binding into the existing Runner configuration:

```yaml
version: 1
agent_instances:
  example: 8c311e35-4bec-4fc4-a6ae-f3654b3ff8b4
```

`example` here is the Agent ID, not the instance display name. The selected instance must belong to the same Runner, installed source, plugin resource and package version as the Agent. Missing, disabled, removed or mismatched instances produce an error rather than falling back to another instance. These UUIDs are operational configuration references, not content to add to task instructions or everyday reports.

Runner pins non-secret configuration and its revision in the Run-local snapshot. `config.get` reads that pinned value, not the latest edited configuration. Credentials are never copied into the snapshot: `credentials.get` reads only a declared slot, on demand, while checking the instance's current availability and permissions. Changing code or ordinary configuration does not replace the pinned execution context: retained version resources and configuration snapshots remain usable for saved jobs and sessions. Unchanged credentials can still be read across a package upgrade; rotating or clearing a credential denies the older invocation access to that slot, and a new authorized invocation is required. Disabling/removing the instance or revoking required permissions blocks further host reads even when a snapshot exists. Revocation does not erase values already handed to trusted local code or rewrite an existing Run's snapshot.

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

## Native interactive environment

A `session_interactive` reply supplies `InteractivePlan {cwd, argv, env}`. Optional
`env` values are references tagged by `source`: `literal`, `inherited`, or
`credential`, with the literal text, local environment variable name, or declared
credential slot in `value`. Credential references are resolved only by the owning
Runner against the selected instance; secret values do not cross RPC in the plan
or enter saved session commands. Wakeit authentication variables cannot be
inherited or overridden through this plan.

An environment-dependent plan can be used by local CLI continuation or the web
terminal, but cannot be exported as a copyable shell command. This preserves the
same boundary as legacy `env`/`env_from` adapters. Runner still validates the exact
Session, working directory, execution scope and ownership lock before launching.

For an unknown Run, `wakeit run reconcile RUN_ID` queues verification by the saved
SDK backend; add `--node NODE_ID` for a DAG node. The REST equivalent is
`POST /runs/{execution_id}/reconcile`. `ctx.runs.reconcile(runId)` uses
this observation path; `runs.resolve` remains the separate manual `{state,reason}`
confirmation. Neither reconciliation nor offline/restart recovery resubmits
Agent work. Unsupported or uncertain observations leave the Run unknown.
