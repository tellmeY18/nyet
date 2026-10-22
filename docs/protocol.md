# Protocol

Status: draft. The protocol is not implemented yet. The CLI calls `nyet-core` directly. The wire layer and the `nyet-protocol` crate are added when a second client exists; this document describes the target contract, not current behavior.

The current CLI apply path uses a plan id plus an interactive confirmation (or `--yes`); the confirmation token described below is part of the future protocol, not the MVP CLI.

## Transport

- JSON-RPC 2.0.
- Unix domain socket, default `$XDG_RUNTIME_DIR/nyet/engine.sock`.
- One request per line or length-prefixed frames; choose one and keep it stable.
- A client may subscribe to server-sent events over the same connection.
- Peer credentials identify the local user. Do not trust a user-supplied identity field.

A D-Bus adapter may expose a subset for desktop integration, but the Unix socket API remains authoritative.

## Client contract

The protocol is intent-only.

- Clients read the option model, current values, plans, and transactions.
- Clients express intents on option paths.
- There is no enable, install, start, rebuild, or shell-out method. Write-side behavior is always `plan.create` then `plan.apply`.
- Package changes are list-membership intents, not an install command.
- The engine owns all Nix evaluation and rendering. Clients never send a whole configuration or a shell command.

The same contract holds for the in-process CLI: the library does not expose imperative operations.

## Versioning

Every response includes the engine version. `engine.capabilities` reports:

- Protocol version.
- Engine version.
- Supported value types.
- Supported operations.
- Feature flags such as `provenance`, `rollback`, and `package-search`.

Clients must fail gracefully when a capability is absent.

## Methods

### `engine.capabilities`

Returns static and dynamic capabilities.

### `options.index`

Returns a paginated option tree or flat list.

Parameters:

```json
{
  "query": "openssh",
  "prefix": "services",
  "include_internal": false,
  "cursor": null,
  "limit": 100
}
```

The index should be cheap enough to populate a UI. Full descriptions and definitions can be fetched lazily.

### `options.get`

Returns one option with metadata, current value, default, and provenance.

```json
{
  "path": "services.openssh.enable",
  "include_definitions": true
}
```

### `config.evaluate`

Evaluates selected paths or a small set of UI-relevant paths.

```json
{
  "paths": [
    "services.openssh.enable",
    "networking.hostName"
  ]
}
```

Do not return the entire evaluated configuration by default.

### `intents.set`

Sets a pending desire. This does not write anything.

```json
{
  "intents": [
    {
      "op": "set",
      "path": "services.openssh.enable",
      "value": {"type": "bool", "value": true}
    },
    {
      "op": "list_add",
      "path": "environment.systemPackages",
      "value": {"type": "package", "value": "pkgs.ripgrep"}
    }
  ]
}
```

Intent operations are `set`, `reset`, `list_add`, and `list_remove`. There is no `enable`, `install`, or `start` operation.

### `intents.clear`

Clears one or all pending intents.

### `plan.create`

Creates an immutable plan from the current intents.

```json
{
  "description": "Enable SSH"
}
```

The result includes a plan identifier, a diff, warnings, and a confirmation token.

### `plan.get`

Returns a plan by identifier.

### `plan.apply`

Applies a plan.

```json
{
  "plan_id": "pln_...",
  "confirmation_token": "..."
}
```

The plan identifier and token are both required. Applying an expired or already-used plan fails.

### `transaction.list`

Returns recent transactions.

### `transaction.rollback`

Rolls back to a recorded generation, subject to helper policy and confirmation.

### `events.subscribe`

Subscribes to events:

- `plan.progress`
- `build.output`
- `build.finished`
- `transaction.finished`
- `engine.warning`

Events are advisory for display; the authoritative state is always fetched from the engine.

## Value representation

Use tagged JSON objects:

```json
{"type": "bool", "value": true}
{"type": "string", "value": "example"}
{"type": "int", "value": 22}
{"type": "enum", "value": "strict"}
{"type": "package", "value": "pkgs.openssh"}
{"type": "expr", "value": "pkgs.callPackage ./foo.nix {}"}
```

The `expr` type is an escape hatch. Frontends must not render it as a normal text field without a warning.

## Errors

Use JSON-RPC error objects with stable codes and a human-readable message.

Suggested application codes:

- `-32001` unknown option.
- `-32002` invalid value.
- `-32003` read-only option.
- `-32004` conflicting definition.
- `-32005` stale plan.
- `-32006` confirmation required.
- `-32007` build failed.
- `-32008` privilege denied.
- `-32009` transaction in progress.
- `-32010` unsupported capability.

Errors may include structured `data` for frontends, but must not leak secrets.

## Security notes

- Never accept a filesystem path from a frontend for a write.
- Never accept a whole configuration file from a frontend for a rebuild.
- The helper receives a plan identifier, not a command line full of Nix.
- Validate option paths against the evaluated option index before planning.
- Validate value types and size limits before storing intents.
- Rate-limit plan creation and builds per user.
- Expire plans and confirmation tokens.
- Record the actor and plan in every transaction.

## Open protocol questions

- Line-delimited JSON versus length-prefixed frames.
- When to replace the spawn-per-call CLI path with a long-running user service.
- How to stream large build logs without blocking commands.
- How to represent option submodules and nested lists in a UI-friendly way.
- Whether `plan.apply` should return immediately with a transaction ID or wait for completion.
