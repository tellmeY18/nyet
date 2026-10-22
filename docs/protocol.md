# Protocol

Status: draft. The protocol is not implemented yet. The CLI calls `nyet-core` directly and the current write path uses the managed-module MVP.

The wire layer and the `nyet-protocol` crate are part of the target architecture because software stores are the primary clients. This document describes the target contract, not current behavior.

The current CLI apply path uses a plan id plus an interactive confirmation (or `--yes`); the confirmation token described below is part of the future protocol.

## Transport

- JSON-RPC 2.0.
- Unix domain socket, default `$XDG_RUNTIME_DIR/nyet/engine.sock`.
- One request per line or length-prefixed frames; choose one and keep it stable.
- A client may subscribe to server-sent events over the same connection.
- Peer credentials identify the local user. Do not trust a user-supplied identity field.

A D-Bus adapter may expose a subset for desktop integration, but the Unix socket API remains authoritative.

## Client contract

The protocol is intent-only.

- Clients read the repository model, option model, current values, plans, and transactions.
- Clients express intents on option paths, package lists, and explicit definition targets.
- There is no enable, install, start, rebuild, or shell-out method. Write-side behavior is always `plan.create` then `plan.apply`.
- Package changes are list-membership intents, not an install command.
- The engine owns all repository discovery, parsing, Nix evaluation, edit rendering, migration, and activation.
- Clients never send a filesystem path for a write and never send a whole configuration.
- A migration is a plan with an explicit confirmation, not an implicit side effect.

The same contract holds for the in-process CLI: the library does not expose imperative operations.

## Versioning

Every response includes the engine version. `engine.capabilities` reports:

- Protocol version.
- Engine version.
- Supported value types.
- Supported plan kinds.
- Feature flags such as `repo_scan`, `repo_migration`, `provenance`, `edits`, `package_search`, `tiers`, and `rollback`.

Clients must fail gracefully when a capability is absent.

## Methods

### `engine.capabilities`

Returns static and dynamic capabilities.

### `repo.scan`

Discovers the configuration root and builds or refreshes the repository index.

Parameters: none. The engine uses its configured root; a client cannot select an arbitrary path.

Returns:

- Root kind: `flake` or `classic`.
- Host names and selected host.
- Standard-layout status.
- Files, imports, and definition counts.
- Migration status and warnings.
- Repository fingerprint.

Read-only scanning must work on an unmigrated repository.

### `repo.migration.plan`

Creates a migration plan for the discovered root.

The plan contains the full diff, affected files, before-hashes, candidate evaluation result, warnings, and confirmation token.

### `repo.migration.apply`

Applies a migration plan.

```json
{
  "plan_id": "pln_...",
  "confirmation_token": "..."
}
```

Migration changes repository structure only. It does not activate a system generation. The result records a migration snapshot that can restore the exact original files.

### `options.index`

Returns a paginated option tree or flat list.

Parameters:

```json
{
  "query": "openssh",
  "prefix": "services",
  "include_internal": false,
  "tier": "basic",
  "cursor": null,
  "limit": 100
}
```

The index should be cheap enough to populate a UI. Full descriptions and definitions are fetched lazily.

### `options.get`

Returns one option with metadata, current value, default, tier, and provenance.

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

### `config.definitions`

Returns editable definition sites for one option or package list.

```json
{
  "path": "environment.systemPackages"
}
```

Each definition includes an opaque definition id, file display path, range, expression preview, and whether it is safely editable. Clients use the id in intents; they never send a file path for a write.

### `packages.search`

Reads nixpkgs package metadata.

```json
{
  "query": "ripgrep",
  "limit": 50
}
```

Returns package attributes, names, descriptions, versions, and whether they are already present in a configured package list. This is a read operation.

### `intents.set`

Sets pending desires. This does not write anything.

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
      "target": "def_01",
      "value": {"type": "package", "value": "pkgs.ripgrep"}
    }
  ]
}
```

Intent operations are `set`, `reset`, `list_add`, and `list_remove`. There is no `enable`, `install`, or `start` operation. `target` is an optional opaque definition id from `config.definitions`; it is never a path.

When `target` is absent, the engine selects a definition only if the choice is unambiguous. Otherwise the plan fails with an ambiguity error.

### `intents.clear`

Clears one or all pending intents.

### `plan.create`

Creates an immutable plan from the current intents.

```json
{
  "description": "Install ripgrep"
}
```

The result includes a plan identifier, kind, exact file edits, diff, warnings, candidate evaluation, and a confirmation token.

### `plan.get`

Returns a plan by identifier.

### `plan.apply`

Applies a configuration plan and activates it.

```json
{
  "plan_id": "pln_...",
  "confirmation_token": "..."
}
```

The plan identifier and token are both required. Applying an expired, stale, or already-used plan fails.

### `transaction.list`

Returns recent transactions.

### `transaction.rollback`

Rolls back to a recorded generation, subject to helper policy and confirmation.

### `events.subscribe`

Subscribes to events:

- `repo.scan.progress`
- `migration.progress`
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
- `-32011` ambiguous definition.
- `-32012` migration required.
- `-32013` unsafe repository path.

Errors may include structured `data` for frontends, but must not leak secrets.

## Security notes

- Never accept a filesystem path from a frontend for a write.
- Never accept a whole configuration file from a frontend.
- The engine discovers the root. Clients may only act on the discovered repository.
- The helper receives a plan identifier, not a command line full of Nix.
- Validate edit targets against the current repository fingerprint before planning.
- Validate option paths against the evaluated option index before planning.
- Validate value types, file sizes, and edit counts before storing intents.
- Rate-limit plan creation, migrations, and builds per user.
- Expire plans and confirmation tokens.
- Record the actor, plan, migration snapshot, and changed files in every transaction.
- Treat assistant output as untrusted input.

## Open protocol questions

- Line-delimited JSON versus length-prefixed frames.
- When to replace the spawn-per-call CLI path with a long-running user service.
- How to stream large build logs without blocking commands.
- How to represent option submodules and nested lists in a UI-friendly way.
- Whether `plan.apply` should return immediately with a transaction ID or wait for completion.
- How to version curated tier metadata independently of the repository schema.
