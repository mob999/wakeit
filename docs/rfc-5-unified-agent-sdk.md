# Unified Agent plugin contract

Status: accepted after independent contract and execution-boundary review. Extends the generic plugin SDK without changing Task,
Runner configuration or NATS message version 1.

## Outcome and boundaries

A plugin has one public registration model. It can provide Agents, event
handlers, Hooks, actions and query providers together. Official Agent adapters
use exactly the same SDK surface as third-party plugins. They must build without
`wakeit-core`, `wakeit-cli` or `wakeit-gateway`, and do not ship a second set of
Agent YAML definitions.

Manifest metadata describes available capabilities without executing package
code. The SDK registers implementations, and initialization must match the
static declarations. Package source selection, platform artifacts, immutable
versions and stable Agent IDs remain in force.

Public plugin wire types belong to a small independent protocol package, exposed
through the SDK. Core consumes those public contracts; the SDK does not depend
on Core or re-export internal Task/DAG/persistence models. Native protocol
clients, JSON libraries and ordinary language standard libraries remain valid
plugin implementation dependencies.

## Registration and discovery

The canonical manifest entry is `wakeit.plugins`: named plugin resources with a
process entry point, configuration schema, credential slots, permissions and
capability declarations. Agent declarations include stable ID, display name,
logo, parameter schema and supported operations. An Agent is not an action or a
mutable query provider.

Legacy `wakeit.agents`, `wakeit.extensions` and user-authored Agent YAML remain
accepted compatibility inputs. They normalize at the package boundary. The
same resource must not be declared under both canonical and legacy entries.
Official packages use only the canonical entry. The existing source-selection
rules continue to reject ambiguous Agent IDs.

Runner checks declarations against runtime registration before advertising an
Agent. Detection is read-only and never creates a native model session. Missing
native binaries, login requirements, unsupported versions and query errors are
reported truthfully; capability declarations never imply availability.

## Agent operations

The public contract covers the complete current adapter surface:

- detection and the actual native version;
- parameter resolution and actual model catalogs;
- synchronous execution and resumed execution;
- asynchronous submission, job-status queries, cancellation and reconciliation;
- session list/pagination, inspect, rename and native interactive continuation;
- typed public message/tool/log/session/result records, with explicit recording
  incompleteness and bounded transport-safe previews;
- cancellation signal, deadline and confirmed/uncertain process completion.

Execution context contains only the required immutable Run/Task identity,
working directory, resolved input/parameters, selected session/title and plugin
version/configuration context. Acknowledged records are applied in order through a bounded channel, and all
records preceding completion must be persisted before final state is decided.
Returning from RPC is not a completion/cleanup barrier. Native children inherit
the Run process group/Windows Job and cannot detach from host ownership.

The plugin cannot change Run identity, claim,
Runner, working directory, deadline or prior successful attempts. Records never
contain hidden model reasoning.

## Ownership and isolation

Runner retains claim identity, concurrency slots, Journal, state transitions,
output retention, retries, user authorization and NATS delivery. SDK output is
an input to Runner validation, never a `setRunState` API. Agent initialization
never submits native work. Once an execute/submit request may have reached a
plugin, losing IPC is an uncertain execution, not permission to resubmit.

Each Run uses an independent Agent SDK execution process/IPC session. Native
stdin is supplied explicitly from the typed request, never inherited from SDK
RPC stdin; task cwd is explicit rather than the package root. Query operations
have their own bounded process lifetime. Rename and cancel are mutations, not
read-only queries. Cancelling
one Run must not stop another Run or the instance's event/Hook process. Native
children belong to that execution's supervised process tree, including Windows
Job Objects. A plugin response alone cannot prove native process exit. Query
operations use read-only execution contexts and cannot implicitly run a prompt.

Agent records travel on a typed, Run-bound SDK channel. Extension logs,
`events.emit`, UI progress and reading existing Run streams do not substitute
for that channel. Both native process completion and final result semantics
must be retained when Journal writes or pipes are slow or malformed.

SDK-backed Agents and legacy command definitions use distinct, real backend
snapshots (`SdkBackend` versus `LegacyCommand`) and feed the same Runner execution
lifecycle. SDK entries must never invent a placeholder submit command merely to
fit the old `AgentConfig`. Internal snapshots may retain normalized compatibility information;
there is no generated/read-back official YAML and no YAML-to-old-adapter
round-trip masquerading as SDK execution.

## Sessions, recovery and updates

Existing Task Agent IDs and verified native Session IDs remain valid. Session
inspection is independent of prompts, and cwd/ownership/identity checks remain
mandatory. Native titles win; the existing local title mapping remains the
fallback only when no native title exists. Terminal execution is created and
validated by Runner, not by an arbitrary Gateway-provided command.

A running execution fixes source, capability, code version and configuration
revision. Updating a package or disabling an instance does not silently switch
an active Run's implementation. Historical sessions and recovery keep usable
versioned resources. Async recovery uses the saved job/backend and original persisted deadline, not
a fresh timeout or a second execution.prepare pass. Lost native ownership remains `unknown`/uncertain until
reconciled; restarting a plugin must never blindly resubmit an Agent.

## Acceptance requirements

A standalone synthetic plugin that depends only on the public SDK must install,
be discovered as an Agent and execute real Task/DAG assignments without Agent
YAML. Isolated verification must cover concurrent Run cancellation isolation,
streamed records/results, native-start failure, timeout, malformed/oversized
records, credential/config isolation, async job reconciliation and restart
uncertainty, models, session list/inspect/resume/rename, and terminal continuation.

All bundled official Agents must migrate without losing their native argv,
parameter selection, supported query/session operations or output normalization.
Packaging must include compiled plugin resources, not build from source during
installation. Both Rust and TypeScript contracts and schemas must agree. A
dependency check must prove official plugin → SDK → public protocol with no Core
edge. Design and completed implementation require separate independent review.
