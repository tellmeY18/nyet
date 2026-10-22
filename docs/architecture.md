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
- Stage validated apply plans for the helper.
- Model and read transaction records.
- Stream events to clients.

The engine does not write to root-owned paths. It prepares a plan and asks the helper to apply it.

The engine starts as a library inside `nyet-core`, consumed directly by the CLI. A long-running daemon and socket transport are added only when a second client exists; the core API must not depend on them.

### Helper

The helper is a small root process invoked through polkit. It must remain auditable.

Allowed operations:

- Validate a plan id, owner, mode, size, expiry, host, and module grammar.
- Write the generated managed module and wrapper flake to fixed paths.
- Run a fixed `nixos-rebuild switch --flake /var/lib/nyet#<host>`.
- Record a transaction and generation reference.
- Roll back one generation with fixed arguments.

It must not:

- Accept arbitrary paths.
- Accept whole configuration files from a client.
- Evaluate untrusted Nix supplied by a client.
- Run a shell.
- Expose a general-purpose root API.

The helper receives only a plan identifier. The engine stages a plan under `/run/user/<uid>/nyet/plans` (mode `0600`, ten-minute lifetime; `/run/nyet/plans/0` as root) and the helper reads it as root. The helper validates that the file is regular, owned by the authenticated user, and mode `0600` or stricter.

### Frontends

Frontends are clients of the engine protocol. They own presentation and interaction only.

Planned frontends:

- CLI: reference client, scripting surface, debugging tool.
- Qt/QML: first graphical frontend and ztOS integration target.
- GTK4: desktop frontend.
- Web: remote or local browser UI.
- TUI: terminal UI for servers and rescue environments.

No frontend should invoke `nixos-rebuild` or write `/etc/nixos` directly.

### Crate map

The boundaries are enforced by the workspace, not by convention.

Dependency rule: `nyet-cli -> nyet-core`; `nyet-helper -> nothing`.

| Crate | One-sentence contract | Owns | Must never |
|---|---|---|---|
| `nyet-core` (lib) | The only component that understands NixOS configuration: read it, model it, propose changes, render them back as Nix. | Option index and cache; evaluation of values, defaults, provenance, conflicts and warnings; typed `Value` and `OptionPath`; intents; validation; plans and diffs; managed-module rendering; transaction and generation records; structured errors. | Depend on argv, sockets, UI, or root paths; parse Nix text as truth; write to the user's repository. |
| `nyet-cli` (bin `nyet`) | Reference client and debugging surface; a thin interpreter of core calls. | Subcommands, human and JSON output, exit codes; later a client of the engine socket. | Contain configuration logic, evaluate Nix itself, or special-case option semantics. |
| `nyet-helper` (bin `nyet-helper`) | Tiny privileged executor with a fixed, auditable operation set. | Validate plan id, user, mode, size, expiry, and module grammar; read a plan from the user's private runtime directory; atomically write the managed module and wrapper flake at fixed paths; run a fixed rebuild; record transactions; roll back one generation. | Accept paths, content, or commands; link `nyet-core`; evaluate Nix; run a shell. |

`nyet-protocol` (wire types only) is added with the first non-CLI client. It carries no configuration logic.

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
- Operation: set a value, reset to the default, add a list member, or remove a list member.
- Desired value or list member.
- Source client.
- Timestamp.

There is no enable, install, start, or shell-out intent. Package changes are list-membership intents on a list option such as `environment.systemPackages`.

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

## Declarative write model

Every change becomes declarative Nix in the managed module. There is no imperative escape hatch in the core or the protocol.

- A set intent renders `path = value;`.
- A reset intent removes the assignment so the NixOS default applies.
- A list add intent contributes one member to a generated list. NixOS merges lists across modules, so this adds to user-owned lists without rewriting them.
- A list remove intent removes only a member nyet owns; it cannot remove a member defined by another module.
- Multiple intents render deterministically, sorted by path, so plans and diffs are stable.
- A plan diff shows the exact managed-module change and nothing else.
- If the user's own modules set the same option, the plan reports a warning. nyet never silently overrides a hand-written assignment.
- The managed module binds `config`, `lib`, and `pkgs` so package and explicit-expression values resolve.

This is what keeps the repository declarative: nyet edits a generated module, never a live system, and every change can be reviewed as text before it is applied. Package installation is not a command; it is membership in a list.

## Read flow

1. Client calls `engine.capabilities`.
2. Client calls `options.index` or a paginated/filtered variant.
3. Client calls `options.get` or `config.evaluate` for the current values it needs.
4. The engine evaluates against the configured flake or NixOS configuration.
5. Results are cached by flake contents, host, and index schema version.

Evaluation must not block the protocol loop. Long reads run in a worker and stream progress or completion events.

## Write flow

1. Client sets one or more intents.
2. Client requests `plan.create`.
3. Engine validates intents against the option model.
4. Engine renders a candidate managed module.
5. Engine runs a dry-build or evaluation check.
6. Engine returns a plan with a diff, warnings, and a confirmation token.
7. Client calls `plan.apply` with the plan identifier and token.
8. Engine stages the plan in the user runtime directory and passes only the id to the helper.
9. Helper validates the plan and writes the wrapper flake and managed module atomically.
10. Helper runs the fixed rebuild and passes build output through.
11. On success, helper records a transaction and generation and removes the plan.
12. On failure, helper restores the previous module and wrapper; the active generation is unchanged and the transaction records the failure.

## Security boundary

The frontend is untrusted from the helper's point of view.

- The helper is invoked by the engine or CLI, not by a remote frontend.
- The helper receives an opaque plan id, not paths or content.
- The helper accepts only `apply <id>` and `rollback`.
- The helper validates plan ownership, mode, size, expiry, host, and a restricted literal module grammar.
- The helper uses fixed `/etc/nixos` and `/var/lib/nyet` paths and a fixed rebuild command.
- The helper serializes apply and rollback with an exclusive lock.
- The helper clears the environment before invoking `nixos-rebuild`.
- Build output is inherited by the invoking terminal; it is not captured or uploaded.

Polkit identifies the requesting user through `PKEXEC_UID`. The helper verifies that the plan belongs to that user and has not expired. Any user authorized to apply NixOS changes can still select option values that execute code during a build; polkit authentication, not the grammar, is the trust boundary.

## Evaluation strategy

NixOS evaluation is the source of truth. The engine must not infer effective configuration by parsing text.

The engine shells out to `nix eval --json --impure` with embedded expressions that use `builtins.getFlake`. The bulk index carries path, type, description, `defaultText`, flags, and declarations. Arbitrary defaults are not forced in bulk because some NixOS defaults throw; a typed default is fetched per option only when it is safe. Values and defaults are sanitized before serialization: functions become null, derivations become their `outPath`, and paths become strings.

The index is cached under the nyet cache directory. The cache key includes the canonical flake path, the lock file and configuration contents, the host, and an index schema version. A write-through cache is best effort: cache failures never fail a read.

Provenance is fetched for selected paths, not in the bulk index, because forcing `isDefined` on every option can force throwing defaults. Definitions under the flake's nixpkgs input are filtered out; remaining definition files are treated as user or external-module definitions for conflict warnings.

See [ADR 0003](decisions/0003-option-evaluation.md).

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
{ config, lib, pkgs, ... }:
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

## Testing strategy

Core behavior is tested through the library against fixture configurations, never the developer's host. Rendering and plan diffs are protected by golden tests. Privileged behavior is kept behind the helper boundary and verified in a Linux VM.

Tests are part of done for every milestone. See [testing.md](testing.md).

## Open design questions

- Exact option evaluation implementation.
- When to add the protocol crate and daemon (trigger: a second client; the CLI calls the library until then).
- How to represent package values and arbitrary expressions.
- How to model multi-host flake configurations.
- How much of Home Manager and nix-darwin to support in the first version.
- Whether nyet should own the flake import or only a module import.
- How to present conflicts and provenance without overwhelming users.
