# Architecture

Status: draft.

## Problem

NixOS configuration is powerful but not approachable. Options are numerous, values are typed but often expressed as Nix, and the only safe write path is to change a module and rebuild the system. Existing graphical tools either embed the whole problem in one UI, write arbitrary files as root, or require the user to understand Nix.

nyet separates the reusable configuration logic from the presentation layer.

## Components

### Engine

The engine is an unprivileged process and library. It is the only component that understands the nyet configuration model.

Responsibilities:

- Discover and cache NixOS options.
- Evaluate current values, defaults, provenance, and warnings.
- Store pending intents.
- Produce a plan and a diff.
- Render the managed Nix module.
- Coordinate dry-builds and apply requests.
- Record transactions and generation references.
- Stream events to clients.

The engine does not write to root-owned paths. It prepares a plan and asks the helper to apply it.

### Helper

The helper is a small root process invoked through polkit. It must remain auditable.

Allowed operations:

- Write the generated managed module to a fixed directory.
- Run a NixOS build or switch with fixed argument construction.
- Record the resulting generation reference.
- Roll back to a recorded generation.

It must not:

- Accept arbitrary paths.
- Accept whole configuration files from a client.
- Evaluate untrusted Nix supplied by a client.
- Run a shell.
- Expose a general-purpose root API.

The helper should receive a plan identifier and read a plan file from a root-owned spool directory. The engine writes the plan into that spool through a narrow, validated handoff.

### Frontends

Frontends are clients of the engine protocol. They own presentation and interaction only.

Planned frontends:

- CLI: reference client, scripting surface, debugging tool.
- Qt/QML: first graphical frontend and ztOS integration target.
- GTK4: desktop frontend.
- Web: remote or local browser UI.
- TUI: terminal UI for servers and rescue environments.

No frontend should invoke `nixos-rebuild` or write `/etc/nixos` directly.

## Data model

### OptionPath

A stable dotted path such as `services.openssh.enable`. The path is the identity of an option.

### OptionSpec

Metadata from NixOS evaluation:

- Path.
- Type.
- Description.
- Default.
- Example.
- Read-only flag.
- Declarations and definitions.
- Visibility and internal flags when available.

### Value

A typed value:

- Boolean.
- Integer.
- Float.
- String.
- Enum.
- Path.
- Package reference.
- List.
- Attribute set.
- Submodule.
- Opaque Nix expression, used only when necessary.

Values must carry their type so a frontend can render the right control and the engine can validate writes.

### Intent

A pending desired change to one option. Intents are not applied until the user creates a plan.

An intent records:

- Option path.
- Desired value.
- Source client.
- Timestamp.
- Whether it is a deletion or reset to default.

### Plan

An immutable description of what would change.

A plan contains:

- Plan identifier.
- Base generation or configuration revision.
- Intents and resulting diff.
- Affected option paths.
- Build derivation, when available.
- Warnings and validation errors.
- Confirmation token.
- Creation time and expiry.

A plan never mutates the system.

### Transaction

A record of an attempted apply.

A transaction contains:

- Transaction identifier.
- Plan identifier.
- Actor.
- Start and finish times.
- Build log reference.
- Result.
- Resulting generation.
- Rollback generation.

### Generation

A NixOS system generation reference. Rollback targets a recorded generation, not a reconstructed configuration.

## Read flow

1. Client calls `engine.capabilities`.
2. Client calls `options.index` or a paginated/filtered variant.
3. Client calls `options.get` or `config.evaluate` for the current values it needs.
4. The engine evaluates against the configured flake or NixOS configuration.
5. Results are cached by configuration revision and nixpkgs revision.

Evaluation must not block the protocol loop. Long reads run in a worker and stream progress or completion events.

## Write flow

1. Client sets one or more intents.
2. Client requests `plan.create`.
3. Engine validates intents against the option model.
4. Engine renders a candidate managed module.
5. Engine runs a dry-build or evaluation check.
6. Engine returns a plan with a diff, warnings, and a confirmation token.
7. Client calls `plan.apply` with the plan identifier and token.
8. Engine hands the plan to the helper through the validated spool.
9. Helper writes the managed module and runs the rebuild.
10. Engine streams build output as events.
11. On success, engine records a transaction and generation.
12. On failure, the previous generation remains active; the transaction records the failure.

## Security boundary

The frontend is untrusted from the helper's point of view.

- The helper is invoked by the engine, not by a frontend.
- The helper receives opaque identifiers, not paths or content.
- The helper validates every path against a fixed allowlist.
- The helper runs one operation at a time under a lock.
- The helper does not inherit a shell or user-controlled environment.
- Build output is returned to the engine; it is not blindly executed.

Polkit identifies the requesting user. The helper must verify that the plan belongs to that user and has not expired.

## Evaluation strategy

NixOS evaluation is the source of truth. The engine must not infer effective configuration by parsing text.

Expected inputs:

- The user's flake or classic NixOS configuration path.
- The host name for flake-based configurations.
- The nixpkgs revision and lock file state.

Expected outputs:

- Option metadata.
- Current values for requested paths.
- Defaults.
- Definitions and declaration locations for provenance.
- Evaluation warnings and errors.

Full configuration JSON is not a good protocol payload: it can be huge and may contain non-serializable values. Prefer an option index plus targeted value queries, with a bounded bulk-read operation for the first screen.

Open questions:

- Which evaluation mechanism is fastest and most reliable across flakes, channels, and older NixOS releases?
- How much provenance can be extracted without making every read prohibitively slow?
- How should values that are functions, derivations, or packages be represented to frontends safely?

## Managed module

nyet writes a dedicated module, for example:

```nix
{ ... }:
{
  services.openssh.enable = true;
}
```

The module is imported by the user's configuration. It is not the whole configuration. The exact path and import mechanism must be explicit in the initial setup flow.

Rules:

- Preserve the user's other files.
- Make generated content deterministic and formatted.
- Record which options nyet owns.
- Handle conflicts when the user sets the same option elsewhere.
- Never silently overwrite a hand-written assignment.

## Failure handling

- Evaluation failure: return structured errors, keep the previous plan.
- Dry-build failure: keep the plan for inspection; do not apply.
- Apply failure: keep the previous generation; record the failed transaction and logs.
- Interrupted apply: on next startup, reconcile the transaction record with the active generation.
- Corrupt state: fail closed and report, rather than guessing.

## Open design questions

- Exact option evaluation implementation.
- Whether the engine exposes a daemon, a library, or both.
- How to represent package values and arbitrary expressions.
- How to model multi-host flake configurations.
- How much of Home Manager and nix-darwin to support in the first version.
- Whether nyet should own the flake import or only a module import.
- How to present conflicts and provenance without overwhelming users.
