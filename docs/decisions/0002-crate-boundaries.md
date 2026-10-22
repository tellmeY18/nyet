# ADR 0002: Crate boundaries and the intent-only client contract

- Status: accepted
- Date: 2026-10-22

## Context

ADR 0001 chose a headless engine with multiple frontends. The repository now has three crates, but the boundaries and the client contract were only implied by prose. Without an explicit rule, the workspace can drift into a monolith, the helper can accumulate engine dependencies, and clients can be tempted to ask for imperative operations.

The product goal is one local engine that any client can call to inspect an existing NixOS configuration, browse the evaluated option surface, and translate every change into declarative Nix.

## Decision

Keep exactly three crates for now, with a hard dependency rule:

```text
nyet-cli -> nyet-core
nyet-helper -> nothing
```

| Crate | Contract |
|---|---|
| `nyet-core` (lib) | The only component that understands NixOS configuration. Owns the option index and cache, evaluation, typed values, intents, validation, plans, diffs, managed-module rendering, and transaction records. Does not depend on argv, sockets, UI, or root paths, and does not write to the user's repository. |
| `nyet-cli` (bin `nyet`) | A thin reference client. Owns argument parsing, output, and exit codes. Contains no configuration logic and does not evaluate Nix. |
| `nyet-helper` (bin `nyet-helper`) | A tiny privileged executor. Accepts a plan id and a fixed operation, validates the plan, user, and expiry, writes the managed module at a fixed path, runs a fixed rebuild, records the generation, and rolls back. Accepts no paths, content, or commands, and links no engine code. |

Additional rules:

- The client contract is intent-only. Clients read the model and express intents on options. There is no enable, install, start, rebuild, or shell-out operation.
- Package changes are list-membership intents on a list option, not an install command.
- Every intent renders as declarative Nix in the managed module.
- Do not add a shared utility crate. Small duplication is cheaper than a fourth boundary.
- Do not add `nyet-protocol` or a daemon until a second client exists. The CLI calling the library directly is the test that the core API is clean.

## Consequences

Positive:

- The privileged binary stays small and auditable.
- The engine API is forced to be usable from outside the CLI before a socket exists.
- Clients in any language share one semantic model: read, then intent.
- The declarative guarantee is structural, not a convention: there is no imperative method to call.

Negative:

- Three manifests and a workspace to maintain.
- The CLI must be refactored when the socket layer arrives.
- Duplication is accepted rather than factored into a shared crate.

## Alternatives considered

### Single crate with three binaries

Rejected. It would let the helper link evaluation and parsing code, which is the largest privilege risk in the project.

### One `nyet` binary with a `helper` subcommand

Rejected. Polkit targets a fixed executable; a multicall binary widens the root attack surface and weakens auditability.

### Add `nyet-protocol` now

Rejected for now. There is no second client, so the wire format would be designed against guesses. The core API is the contract until the CLI proves it.

### Shared utility crate

Rejected. It invites cycles and makes the helper depend on unrelated code.

## Follow-ups

- Build the POC vertical slice against a fixture.
- Keep the helper dependency-free as it grows.
- Revisit this ADR when the second client is scheduled.
