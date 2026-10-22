# AGENTS.md

This file is the working context for agents and maintainers in this repository. Read it before changing code or architecture.

## Mission

nyet is a headless NixOS configuration engine with multiple frontends.

It must let a user inspect NixOS options, change supported values safely, preview the result, apply it, and roll it back without exposing Nix or root operations to the frontend.

The product goal is one local engine that any client can call to:

- Analyze the user's existing NixOS configuration and show its structure.
- Browse the full evaluated option surface, including services and programs.
- Turn every requested change into declarative Nix, never imperative state.
- Prepare, preview, apply, and roll back changes safely.

Clients connect to the engine; they never evaluate Nix, edit files, or perform privileged operations themselves.

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

The M1 read model, M2 intent/plan model, and an MVP M3 apply path work against a fixture configuration and a fixed `/etc/nixos` target. The apply and rollback path has not been exercised on a disposable NixOS VM yet.

Implemented:

- Project context, architecture, protocol, integration, testing, and roadmap documents.
- Crate contracts and the intent-only client model (ADR 0002).
- Option evaluation through `nix eval` against a fixture flake (ADR 0003).
- Typed `OptionPath`, `OptionSpec`, and `Value`.
- Cached option index keyed by the flake path, configuration contents, host, and schema version.
- CLI read path: `nyet options` and `nyet get`, including selected-path provenance.
- Persistent intents under the nyet state directory, including set, reset, list add, and list remove.
- Validation against option type metadata, read-only flags, enums, and list element types.
- Deterministic managed-module rendering and line diffs.
- Plan warnings when a selected option is also defined outside nixpkgs.
- Candidate-module evaluation check: `nyet plan --check`.
- CLI: `options`, `get`, `set`, `unset`, `add`, `remove`, `intents`, `plan`, `apply`, `rollback`, and `history`.
- Plan staging under the user runtime directory with ownership, mode, size, expiry, and module-grammar validation in the helper.
- Privileged helper: fixed plan ID, fixed `/etc/nixos` input, wrapper flake at `/var/lib/nyet/flake.nix`, managed module at `/var/lib/nyet/nyet.nix`, fixed `nh os switch /var/lib/nyet#<host>`, transaction records under `/var/lib/nyet/transactions`, and one-step rollback.
- `packages.default`, `nixosModules.default`, and a polkit rule requiring admin authentication for `nyet-helper`.
- 65 tests: core unit, helper unit, fixture evaluation, golden render, and CLI integration.
- A gated x86_64-linux NixOS VM test for apply and rollback, run by GitHub Actions.
- Nix dev shell.

Not implemented:

- A green CI run of the VM test; it has not been exercised on a disposable NixOS VM yet.
- Actual NixOS dry-build on the target platform (the current check evaluates `system.build.toplevel.drvPath`).
- First-run setup, automatic managed-module import, and multi-host selection.
- Build-log capture and rollback to an arbitrary recorded generation.
- Protocol, daemon, and `nyet-protocol`.
- Any GUI frontend.
- Integration with ztOS.

Do not describe unbuilt pieces as working.

## Crate responsibilities

Keep the workspace small and the boundaries hard. Dependency rule: `nyet-cli -> nyet-core`; `nyet-helper -> nothing`. No shared utility crate, no cycles, no UI in core, no Nix in the helper.

| Crate | One-sentence contract | Owns | Must never |
|---|---|---|---|
| `nyet-core` (lib) | The only component that understands NixOS configuration: read it, model it, propose changes, render them back as Nix. | Option index and cache; evaluation of values, defaults, provenance, conflicts and warnings; typed `Value` and `OptionPath`; intents; validation; plans and diffs; managed-module rendering; transaction and generation records; structured errors. | Depend on argv, sockets, UI, or root paths; parse Nix text as truth; write to the user's repository. |
| `nyet-cli` (bin `nyet`) | Reference client and debugging surface; a thin interpreter of core calls. | Subcommands, human and JSON output, exit codes; later a client of the engine socket. | Contain configuration logic, evaluate Nix itself, or special-case option semantics. |
| `nyet-helper` (bin `nyet-helper`) | Tiny privileged executor with a fixed, auditable operation set. | Validate plan id, user, mode, size, expiry, and module grammar; read a plan from the user's private runtime directory; atomically write the managed module and wrapper flake at fixed paths; run a fixed rebuild; record transactions; roll back one generation. | Accept paths, content, or commands; link `nyet-core`; evaluate Nix; run a shell. |

The client contract is intent-only. A client may read the model and express intents on options; it may not ask for an imperative operation. Package changes are list-membership intents, not an `install` command.

The wire layer (JSON-RPC over a Unix socket) is a later adapter over the same core API. Do not add `nyet-protocol` or an engine daemon until a second client exists; the CLI is the test that the core API is clean.

## Decisions already made

- Product name: **nyet**. Do not rename it without an explicit user decision.
- License: **GPL-3.0-or-later**.
- The engine is headless and language-neutral at the protocol boundary.
- The protocol is JSON-RPC over a Unix domain socket. A D-Bus adapter can be added later, but must not be the only transport.
- NixOS evaluation is the source of truth. The engine uses `nix eval --json --impure` with embedded expressions (ADR 0003).
- The engine writes a managed module such as `nyet.nix`; user configuration remains hand-written and authoritative elsewhere.
- Privilege is split between an unprivileged engine and a narrow polkit-backed helper.
- The helper accepts plan identifiers and fixed operations, never arbitrary paths or whole config files from a client.
- The first frontend should be the CLI, to force the engine API to be clean. A Qt/QML frontend is the likely first graphical frontend because ztOS is the first integration target. GTK can follow.
- The engine should be implemented in Rust. Existing NixOS tooling and `nix-editor` are Rust-oriented, and a single native engine can serve multiple frontends.
- Start with the read-only option explorer before write-back.
- The engine starts as a library consumed directly by the CLI. A daemon and the `nyet-protocol` crate are added only when a second client exists.
- Changes are expressed as intents on options, never as imperative commands such as enable, install, or start.
- Extensive automated tests are part of the definition of done for every milestone, not a later cleanup.

## Hard rules

1. **Do not use a Nix parser as the source of truth.** Evaluate the module system. Text editing is only for rendering the managed module and preserving user formatting where applicable.
2. **Do not give the helper arbitrary path or content input.** A compromised frontend must not become arbitrary root file write or arbitrary command execution.
3. **Do not pass full configuration files through command-line arguments.** Use stdin, file descriptors, or a private, validated spool file.
4. **Do not write outside the managed module and nyet state directories without an explicit design decision.**
5. **One transaction at a time.** Use locking and a durable transaction record.
6. **Do not add a root daemon unless the design is reviewed.** Prefer an unprivileged engine plus a short-lived polkit helper.
7. **Do not use `nix profile`.** Use flakes and dev shells.
8. **Do not commit secrets or user-specific host data.**
9. **Keep the engine free of UI toolkit dependencies.**
10. **Update the relevant document when changing an architectural invariant.**
11. **Do not commit unless the user explicitly asks.**
12. **Clients express intents, never imperative operations.** No enable, install, start, or shell-out API. Every change renders as declarative Nix in the managed module.
13. **Every behavior gets tests.** Nix-backed behavior is tested against fixture configurations, not the developer's host.

## Planned layout

```text
crates/
  nyet-core/       domain types, option index, intents, plans, transactions
  nyet-cli/        CLI frontend and debugging surface
  nyet-helper/     privileged helper; fixed operations only
  nyet-protocol/   JSON-RPC types and schema (planned; only after a second client)
tests/
  fixtures/        fixture flakes and NixOS configurations
  golden/          expected rendered modules and diffs
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

## State and cache

- Pending intents live under `$XDG_STATE_HOME/nyet` or `~/.local/state/nyet`; override with `NYET_STATE_DIR`.
- The option index cache lives under `$XDG_CACHE_HOME/nyet` or `~/.cache/nyet`; override with `NYET_CACHE_DIR`.
- Apply plans are staged under `/run/user/<uid>/nyet/plans` (or `/run/nyet/plans/0` as root) with mode `0600` and a ten-minute lifetime.
- Applied state is fixed: `/var/lib/nyet/nyet.nix`, `/var/lib/nyet/nyet.nix.previous`, `/var/lib/nyet/flake.nix`, and `/var/lib/nyet/transactions`.
- None of these are written inside the user's flake or repository.
- The cache is best effort and keyed by flake contents, host, and schema version.

## Testing

Tests are part of done, not a follow-up. Add them with the behavior they protect.

- Unit tests live next to the code.
- Nix-backed tests run against fixture configurations under `tests/fixtures/`, never the developer's host.
- Rendered modules and plan diffs are protected by golden tests.
- Default tests do not mutate a real configuration and do not require root.
- Linux-only NixOS VM tests are gated and run in CI.

See [docs/testing.md](docs/testing.md).

## Protocol invariants

- JSON-RPC 2.0 over a Unix socket.
- The wire layer is not implemented yet. Until a second client exists, the CLI calls `nyet-core` directly.
- Clients may read the model and set intents; there is no imperative operation.
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
- The helper has a small, auditable command set: write the managed module and wrapper, run a fixed rebuild, record a transaction, roll back one generation.
- The helper validates plan ownership, mode, size, expiry, host, and a restricted literal module grammar before writing.
- The helper does not evaluate untrusted Nix supplied by a client and does not accept paths or commands.
- The helper uses fixed `/etc/nixos` and `/var/lib/nyet` paths; the wrapper flake extends the user's host configuration via `extendModules`.
- The plan and transaction identifiers are unguessable and tied to the requesting user.
- Build logs may contain sensitive host information; do not upload them by default.

## Prior art

The closest project is `snowfallorg/nixos-conf-editor`. It is a GTK4 application, GPL-3.0, with a thin root helper. It uses `nix-editor` to parse and rewrite configuration text and calls `pkexec nce-helper write-rebuild --content ... --path ...`. It has no reusable engine API and passes whole config content and paths to a root helper. Use it as prior art, not as the architecture.

See [docs/prior-art.md](docs/prior-art.md).

## Next steps

1. Add NixOS VM integration tests for apply and rollback.
2. Add first-run setup and explicit managed-module/wrapper installation.
3. Add build-log capture and rollback to an arbitrary recorded generation.
4. Add the first graphical frontend after the CLI flow is verified end to end.
5. Integrate with ztOS as a flake input only after the standalone flow works.

## Working style

- Keep changes small and reviewable.
- Prefer explicit domain types over loosely typed JSON in the core.
- Add tests next to the behavior they protect.
- Document open questions in ADRs rather than leaving them implicit.
- Do not claim a feature works until it has been exercised end to end.
