# ADR 0001: Headless engine with multiple frontends

- Status: accepted; amended by ADR 0005
- Date: 2026-10-22
- Amendment: ADR 0005 replaces the managed-module-only write model with repository-aware editing. Nix parsing is allowed for structure and provenance; evaluation remains the source of truth. Software-store clients require the service and protocol.

> The original decision text below is retained for history. Where it conflicts with ADR 0005 or ADR 0006, the newer ADR wins.

## Context

nyet needs to support a CLI, a Qt/QML frontend for ztOS, a GTK4 desktop frontend, and potentially web and TUI clients. Embedding configuration logic in each frontend would duplicate evaluation, validation, privilege handling, and transaction logic.

The closest prior art, `nixos-conf-editor`, is a GTK application with a thin root helper. Its helper accepts configuration content and a path from the GUI. That is not a suitable reusable or security architecture.

## Decision

Build a headless engine and make every frontend a protocol client.

- The engine is unprivileged and owns evaluation, intents, plans, transactions, and managed module generation.
- The protocol is JSON-RPC over a Unix socket.
- A narrow polkit-backed helper performs privileged writes and rebuilds using plan identifiers and fixed paths.
- The CLI is the reference client and is built first.
- The Qt/QML frontend is the first graphical client because ztOS is the first integration target.
- NixOS evaluation is the source of truth; nyet does not parse Nix to discover effective configuration.
- nyet writes a managed module rather than rewriting user configuration.
- The engine is written in Rust.

## Consequences

Positive:

- One implementation of evaluation, validation, and security.
- Frontends can use different toolkits and languages.
- The CLI provides a testable, scriptable surface.
- The helper can be audited independently.
- ztOS can consume nyet without embedding engine logic.

Negative:

- More up-front design work than a single GTK app.
- Protocol versioning and compatibility become ongoing concerns.
- The engine must be packaged and run consistently. It starts as a library consumed by the CLI; the service arrives with the second client.
- A privileged helper still requires careful review.

## Alternatives considered

### Fork nixos-conf-editor directly

Rejected as the primary architecture. It provides a GTK frontend, not a reusable engine, and its privileged path is too broad. It remains useful as prior art and as a source of UI ideas.

### Put everything in a root daemon

Rejected. A root daemon that evaluates arbitrary Nix and accepts client requests has a large attack surface.

### Build only a CLI

Rejected. The product goal includes graphical frontends and ztOS integration.

### Build only a Qt frontend

Rejected. This would reproduce the coupling problem and make other clients expensive.

## Follow-up decisions

- Exact evaluation mechanism.
- Protocol framing.
- Managed module path and import strategy.
- Helper spool format.
- First-run setup flow.
- Which option types are editable in the first release.

Crate boundaries and the intent-only client contract are decided in [ADR 0002](0002-crate-boundaries.md). The POC evaluation mechanism is decided in [ADR 0003](0003-option-evaluation.md).
