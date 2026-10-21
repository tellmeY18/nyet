# AGENTS.md

This file is the working context for agents and maintainers in this repository. Read it before changing code or architecture.

## Mission

nyet is a headless NixOS configuration engine with multiple frontends.

It must let a user inspect NixOS options, change supported values safely, preview the result, apply it, and roll it back without exposing Nix or root operations to the frontend.

The engine, not the UI, owns:

- Nix evaluation and option discovery.
- Current values, defaults, provenance, and warnings.
- Pending intents and plans.
- Managed module generation.
- Build, apply, transaction, and rollback state.

Frontends are replaceable clients.

## Non-goals

- nyet is not a Nix parser. Do not build a general Nix parser.
- nyet is not a replacement for the NixOS module system.
- nyet does not maintain a parallel database of effective system configuration.
- nyet does not rewrite arbitrary user Nix files.
- nyet is not a package store. Package installation can be a provider or a separate frontend later.

## Current state

This repository is a new scaffold. There is no functional engine yet.

Implemented:

- Project context and architecture documents.
- Protocol draft.
- Minimal Rust workspace with `nyet-core`, `nyet-cli`, and `nyet-helper`.
- Nix dev shell.

Not implemented:

- Option evaluation.
- Intent storage.
- Plan generation.
- Privileged helper operations.
- Any GUI frontend.
- Integration with ztOS.

Do not describe unbuilt pieces as working.

## Decisions already made

- Product name: **nyet**. Do not rename it without an explicit user decision.
- License: **GPL-3.0-or-later**.
- The engine is headless and language-neutral at the protocol boundary.
- The protocol is JSON-RPC over a Unix domain socket. A D-Bus adapter can be added later, but must not be the only transport.
- NixOS evaluation is the source of truth.
- The engine writes a managed module such as `nyet.nix`; user configuration remains hand-written and authoritative elsewhere.
- Privilege is split between an unprivileged engine and a narrow polkit-backed helper.
- The helper accepts plan identifiers and fixed operations, never arbitrary paths or whole config files from a client.
- The first frontend should be the CLI, to force the engine API to be clean. A Qt/QML frontend is the likely first graphical frontend because ztOS is the first integration target. GTK can follow.
- The engine should be implemented in Rust. Existing NixOS tooling and `nix-editor` are Rust-oriented, and a single native engine can serve multiple frontends.
- Start with the read-only option explorer before write-back.

## Hard rules

1. **Do not use a Nix parser as the source of truth.** Evaluate the module system. Text editing is only for rendering the managed module and preserving user formatting where applicable.
2. **Do not give the helper arbitrary path or content input.** A compromised frontend must not become arbitrary root file write or arbitrary command execution.
3. **Do not pass full configuration files through command-line arguments.** Use stdin, file descriptors, or a root-owned spool.
4. **Do not write outside the managed module and nyet state directories without an explicit design decision.**
5. **One transaction at a time.** Use locking and a durable transaction record.
6. **Do not add a root daemon unless the design is reviewed.** Prefer an unprivileged engine plus a short-lived polkit helper.
7. **Do not use `nix profile`.** Use flakes and dev shells.
8. **Do not commit secrets or user-specific host data.**
9. **Keep the engine free of UI toolkit dependencies.**
10. **Update the relevant document when changing an architectural invariant.**
11. **Do not commit unless the user explicitly asks.**

## Planned layout

```text
crates/
  nyet-core/       domain types, option index, intents, plans, transactions
  nyet-cli/        CLI frontend and debugging surface
  nyet-helper/     privileged helper; fixed operations only
  nyet-protocol/   JSON-RPC types and schema (add when the API stabilizes)
frontends/
  qt/              Qt/QML frontend (planned)
  gtk/             GTK4 frontend (planned)
docs/
  decisions/       ADRs
```

The current scaffold has only `nyet-core`, `nyet-cli`, and `nyet-helper`.

## Commands

Use a Nix shell because the development host may not have Rust or Git on `PATH`.

```bash
nix develop
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For Git without a global install:

```bash
nix shell nixpkgs#git --command git status
```

The primary development host is `aarch64-darwin`. It cannot build `aarch64-linux` NixOS images natively. Use Linux or CI for NixOS VM integration tests.

## Protocol invariants

- JSON-RPC 2.0 over a Unix socket.
- The protocol is versioned and capability-discoverable.
- Reads and writes are separate operations.
- `plan` never mutates the system.
- `apply` requires a plan identifier and a confirmation token.
- Build output and progress are streamed as events.
- Values are typed; arbitrary Nix expressions are an explicit escape hatch, not the default representation.
- Errors are structured and stable enough for frontends to display.

See [docs/protocol.md](docs/protocol.md).

## Security invariants

- The engine runs as the invoking user.
- The helper runs as root through polkit.
- The helper has a small, auditable command set: write the managed module, run a rebuild, record a generation reference.
- The helper validates that the target is inside the nyet-managed directory.
- The helper does not evaluate untrusted Nix supplied by a client.
- The plan and transaction identifiers are unguessable and tied to the requesting user.
- Build logs may contain sensitive host information; do not upload them by default.

## Prior art

The closest project is `snowfallorg/nixos-conf-editor`. It is a GTK4 application, GPL-3.0, with a thin root helper. It uses `nix-editor` to parse and rewrite configuration text and calls `pkexec nce-helper write-rebuild --content ... --path ...`. It has no reusable engine API and passes whole config content and paths to a root helper. Use it as prior art, not as the architecture.

See [docs/prior-art.md](docs/prior-art.md).

## Next steps

1. Prove read-only evaluation on a real NixOS configuration: options, values, defaults, provenance.
2. Define the option value model and a curated first-run setting set.
3. Implement the CLI read path against the engine.
4. Implement intents and plan generation without applying anything.
5. Implement the helper with a plan-file handoff and fixed managed-module path.
6. Add the first graphical frontend after the CLI flow works end to end.
7. Add NixOS VM integration tests.
8. Integrate with ztOS as a flake input only after the standalone flow works.

## Working style

- Keep changes small and reviewable.
- Prefer explicit domain types over loosely typed JSON in the core.
- Add tests next to the behavior they protect.
- Document open questions in ADRs rather than leaving them implicit.
- Do not claim a feature works until it has been exercised end to end.
