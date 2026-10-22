# AGENTS.md

This file is the working context for agents and maintainers in this repository. Read it before changing code or architecture.

## Mission

nyet is a headless, repository-aware NixOS configuration engine with multiple frontends.

Its primary audience is people arriving from imperative operating systems. They should be able to install software, change common settings, and configure applications through a software store without first learning Nix. Every change must still become normal declarative Nix in the user's own configuration.

The product goal is one local engine that any client can call to:

- Scan and understand an existing NixOS repository, including its files, imports, hosts, and definitions.
- Browse the full evaluated option surface, including services and programs.
- Turn every requested change into declarative Nix in the repository, never imperative state.
- Prepare, preview, apply, and roll back changes safely.
- Normalize a repository into one supported layout during explicit first-run onboarding.
- Serve software-store clients with package search, install/uninstall intents, and tiered option metadata.

Clients connect to the engine; they never evaluate Nix, edit files, or perform privileged operations themselves.

The engine, not the UI, owns:

- Repository discovery, parsing, provenance, and migration.
- Nix evaluation and option discovery.
- Current values, defaults, provenance, and warnings.
- Pending intents and plans.
- Configuration edits and package-list edits.
- Build, apply, transaction, and rollback state.

Frontends are replaceable clients. The CLI is the reference client; software stores are the primary end-user clients.

## Non-goals

- nyet is not a general Nix parser. Use an existing lossless parser (`rnix`/`rowan` or equivalent); do not implement the grammar.
- nyet is not a replacement for the NixOS module system.
- nyet does not maintain a parallel database of effective system configuration.
- nyet does not edit arbitrary files. It edits a discovered configuration root with explicit targets, snapshots, and validation.
- nyet is not a package manager. Package installation is a list-membership intent; package search reads metadata.
- nyet does not expose imperative operations such as enable, install, start, or shell-out.

## Current state

The implemented code is the M1 read model, the M2 intent/plan model, the managed-module MVP for M3, and the first read-only repository slice for M4. The M4 slice discovers a configuration root, parses `.nix` files with `rnix`, records imports, and renders a classic-to-standard migration plan. It does not apply edits yet. Apply and rollback are covered by a disposable NixOS VM test in CI.

The target architecture in this document is repository-aware editing and migration. Do not describe the parser, migration, package store, protocol, or daemon as working until they exist.

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
- CLI: `options`, `get`, `set`, `unset`, `add`, `remove`, `intents`, `plan`, `apply`, `rollback`, `history`, `scan`, and `migration-plan`.
- Read-only repository discovery and CST parsing with `rnix`: configuration kind, host, file list, relative imports, syntax errors, standard-layout detection, and a classic migration plan.
- Plan staging under the user runtime directory with ownership, mode, size, expiry, and module-grammar validation in the helper.
- Privileged helper: fixed plan ID, fixed `/etc/nixos` input, wrapper flake at `/var/lib/nyet/flake.nix`, managed module at `/var/lib/nyet/nyet.nix`, builds as the requesting user, then runs fixed `nh os switch /var/lib/nyet#<host>` as root, transaction records under `/var/lib/nyet/transactions`, and one-step rollback.
- `packages.default`, `nixosModules.default`, and a polkit rule requiring admin authentication for `nyet-helper`.
- 73 tests on Darwin (74 on Linux): core unit, repository scan and migration-plan tests, helper unit, fixture evaluation, golden render, and CLI integration.
- A green x86_64-linux NixOS VM test for apply and rollback, including a non-root build path, run by GitHub Actions.
- Nix dev shell.

Not implemented:

- Definition indexing, provenance correlation, and in-place edits.
- Applying migration plans.
- In-place editing of existing definitions.
- Package search and software-store flows.
- Option tier metadata and the beginner-first UI model.
- Long-running engine service and `nyet-protocol`.
- Any GUI frontend.
- Build-log capture and rollback to an arbitrary recorded generation.
- Actual NixOS dry-build on the target platform (the current check evaluates `system.build.toplevel.drvPath`).
- SnowflakeOS and ztOS integration.

Do not describe unbuilt pieces as working.

## Crate responsibilities

Keep the workspace small and the boundaries hard. Dependency rule: `nyet-cli -> nyet-core`; `nyet-helper -> nothing`. `nyet-protocol` is planned and depends only on wire types.

| Crate | One-sentence contract | Owns | Must never |
|---|---|---|---|
| `nyet-core` (lib) | The only component that understands NixOS configuration: read it, model it, propose changes, and edit it. | Repository discovery and indexing; CST parsing for structure and edit ranges; option index and cache; evaluation of values, defaults, provenance, conflicts, and warnings; typed `Value` and `OptionPath`; intents; validation; configuration and migration plans; diffs; transaction records; structured errors. | Depend on argv, sockets, UI, or root paths; infer effective configuration from text; write root-owned paths. |
| `nyet-cli` (bin `nyet`) | Reference client and debugging surface; a thin interpreter of core calls. | Subcommands, human and JSON output, exit codes; later a client of the engine socket. | Contain configuration logic, evaluate Nix itself, special-case option semantics, or edit files directly. |
| `nyet-helper` (bin `nyet-helper`) | Tiny privileged executor with a fixed, auditable operation set. | Validate plan id, user, mode, size, expiry, host, relative edit paths, hashes, and module grammar; read a plan from the user's private runtime directory; apply scoped edits under a fixed configuration root; run a fixed rebuild; record transactions; roll back one generation. | Accept arbitrary paths, whole configurations, or commands from a frontend; link `nyet-core`; evaluate Nix; run a shell. |
| `nyet-protocol` (planned) | Wire types and JSON-RPC schema only. | Method and event types, versioning, error codes, capability discovery. | Contain configuration logic, parsing, evaluation, or privilege. |

The client contract is intent-only. A client may read the model and express intents on options, package lists, and explicit edit targets. It may not ask for an imperative operation or send a file path for a write. Package changes are list-membership intents, not an `install` command.

The wire layer is no longer deferred: software-store clients are the primary audience. The service and `nyet-protocol` crate are part of the target architecture, but the CLI still calls `nyet-core` directly until the service slice exists.

## Decisions already made

- Product name: **nyet**. Do not rename it without an explicit user decision.
- License: **GPL-3.0-or-later**.
- The engine is headless and language-neutral at the protocol boundary.
- The protocol is JSON-RPC 2.0 over a Unix domain socket. A D-Bus adapter can be added later, but must not be the only transport.
- NixOS evaluation is the source of truth for effective configuration. Parsing is only for structure, provenance, and edit locations.
- nyet uses an existing lossless Nix parser. It does not build a general Nix parser.
- nyet edits the user's repository, including existing definitions. It does not hide user settings in a nyet-only module.
- First use normalizes the repository into one supported layout. Migration is explicit, snapshotted, evaluated, diffed, and reversible.
- Package install/uninstall is list membership in configuration. `nix profile` and imperative installs are forbidden.
- Software stores are the primary clients. The CLI remains the reference client.
- Options may carry `basic`, `intermediate`, and `advanced` presentation tiers. Tiers never change configuration semantics.
- An LLM or assistant may propose intents or edits, but never writes files directly and never bypasses a plan and confirmation.
- Privilege is split between an unprivileged engine and a narrow polkit-backed helper.
- The helper accepts plan identifiers and fixed operations, never arbitrary paths or whole config files from a client.
- Root-owned repositories are edited only through a scoped helper plan; user-owned repositories may be edited by the engine.
- The engine should be implemented in Rust. Existing NixOS tooling is Rust-oriented, and one native engine can serve multiple frontends.
- The CLI is built before the service and graphical clients so the core API is exercised.
- Changes are expressed as intents on options or package lists, never as imperative commands.
- Extensive automated tests are part of the definition of done for every milestone.

## Hard rules

1. **Evaluation is the semantic source of truth.** Use evaluation for effective values, types, defaults, and conflicts. Never infer effective configuration from text.
2. **Use a CST for edits, not a hand-written parser.** `rnix`/`rowan` or an equivalent library may identify structure and edit ranges; it must not replace evaluation.
3. **Do not give the helper arbitrary path or content input.** A frontend must never become arbitrary root file write or arbitrary command execution. Root-owned edits use a plan scoped to a fixed configuration root with validated relative paths, hashes, and backups.
4. **Do not pass full configuration files through command-line arguments.** Use stdin, file descriptors, or a private, validated spool file.
5. **Write only inside the discovered configuration root, nyet state, backups, and the Nix store.** Every write is atomic and part of a transaction.
6. **One transaction at a time.** Use locking and a durable transaction record.
7. **Do not add a root daemon.** Prefer an unprivileged engine service plus a short-lived polkit helper.
8. **Do not use `nix profile`.** Use flakes and dev shells. Package changes are configuration edits.
9. **Do not commit secrets or user-specific host data.** Do not send secrets to an assistant by default.
10. **Keep the engine free of UI toolkit dependencies.**
11. **Update the relevant document when changing an architectural invariant.**
12. **Do not commit unless the user explicitly asks.**
13. **Clients express intents, never imperative operations.** No enable, install, start, or shell-out API. Every change becomes declarative Nix in the repository.
14. **Every behavior gets tests.** Nix-backed behavior is tested against fixture configurations, not the developer's host.
15. **Migration is explicit and reversible.** Snapshot, diff, evaluate, and record; never rewrite during activation.
16. **Assistant output is a proposal.** Validate it through the normal plan and confirmation path.

## Standard repository layout

After first-run migration, the supported layout is:

```text
flake.nix
hosts/<host>/configuration.nix
hosts/<host>/hardware-configuration.nix
modules/...
```

Rules:

- One flake entrypoint.
- One directory per host.
- Hardware configuration is imported explicitly.
- Shared modules live under `modules/`.
- Existing definitions and imports are preserved; migration normalizes structure, not semantics.
- Read-only scanning must work before migration.

## Code layout

```text
crates/
  nyet-core/       repository index, evaluation, intents, plans, edits, transactions
  nyet-cli/        CLI reference client and debugging surface
  nyet-helper/     privileged helper; fixed operations only
  nyet-protocol/   JSON-RPC types and schema (planned)
tests/
  fixtures/        fixture flakes and NixOS configurations
  golden/          expected edits, migrations, and diffs
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

Target state:

- Pending intents live under `$XDG_STATE_HOME/nyet` or `~/.local/state/nyet`; override with `NYET_STATE_DIR`.
- The repository index and option cache live under `$XDG_CACHE_HOME/nyet` or `~/.cache/nyet`; override with `NYET_CACHE_DIR`.
- Migration snapshots and transaction records live under `/var/lib/nyet` for root-owned repositories and the nyet state directory for user-owned repositories.
- Apply plans are staged under `/run/user/<uid>/nyet/plans` (or `/run/nyet/plans/0` as root) with mode `0600` and a ten-minute lifetime.
- The repository is the declarative source of truth. Nyet state contains plans, snapshots, transactions, and caches, never the effective configuration.

Current MVP state:

- The managed module is `/var/lib/nyet/nyet.nix`, the wrapper is `/var/lib/nyet/flake.nix`, and the previous module is `/var/lib/nyet/nyet.nix.previous`.
- These paths are transitional and are replaced by repository edits after migration support lands.

## Testing

Tests are part of done, not a follow-up. Add them with the behavior they protect.

- Unit tests live next to the code.
- Nix-backed tests run against fixture configurations under `tests/fixtures/`, never the developer's host.
- Repository scanning and CST edits are tested against fixture repositories.
- Migration tests cover flake and classic layouts, preservation of comments and definitions, idempotency, ambiguity refusal, and exact rollback.
- Golden tests protect rendered edits, migration output, and plan diffs.
- Default tests do not mutate a real configuration and do not require root.
- Linux-only NixOS VM tests are gated and run in CI.

See [docs/testing.md](docs/testing.md).

## Protocol invariants

- JSON-RPC 2.0 over a Unix socket.
- The service is not implemented yet. The CLI calls `nyet-core` directly until it exists.
- Clients may read the model, scan the repository, request a migration plan, and set intents; there is no imperative operation.
- The protocol is versioned and capability-discoverable.
- Reads and writes are separate operations.
- `plan` never mutates the system or the repository.
- `apply` requires a plan identifier and a confirmation token.
- Repository and build output are streamed as events.
- Values are typed; arbitrary Nix expressions are an explicit escape hatch, not the default representation.
- Clients do not send filesystem paths for writes. The engine discovers the root and selects edit targets.
- Errors are structured and stable enough for frontends to display.

See [docs/protocol.md](docs/protocol.md).

## Security invariants

- The engine runs as the invoking user.
- The helper runs as root through polkit.
- The helper has a small, auditable command set: apply a scoped repository or migration plan, run a fixed rebuild, record a transaction, roll back one generation.
- The helper validates plan ownership, mode, size, expiry, host, plan kind, relative paths, before-hashes, and module grammar before writing.
- Relative edit paths are canonicalized under one fixed configuration root; traversal, symlink escapes, and unexpected file types are rejected.
- The helper does not evaluate untrusted Nix supplied by a client and does not accept commands.
- The engine prepares edits; it does not write root-owned paths.
- Migration snapshots are retained until the transaction is committed and are used for exact rollback.
- The plan and transaction identifiers are unguessable and tied to the requesting user.
- Build logs may contain sensitive host information; do not upload them by default.
- Assistant proposals are untrusted input and follow the same validation and confirmation path.

## Prior art

The closest project is `snowfallorg/nixos-conf-editor`. It is a GTK4 application, GPL-3.0, with a thin root helper. It uses `nix-editor` to parse and rewrite configuration text and calls `pkexec nce-helper write-rebuild --content ... --path ...`. It is useful prior art for option UI and text editing, but it has no reusable engine API and its helper accepts whole config content and paths.

See [docs/prior-art.md](docs/prior-art.md).

## Next steps

1. Keep the repository-aware architecture documents and ADRs current.
2. Finish repository indexing: definitions, edit ranges, provenance correlation, and ambiguity rules.
3. Apply the migration plan atomically, with snapshots, evaluation, diffs, and rollback.
4. Add in-place setting edits, package-list edits, and tier metadata.
5. Add the unprivileged engine service, `nyet-protocol`, and the first software-store client.
6. Add the first graphical frontend after the service contract is exercised.
7. Integrate with SnowflakeOS/ztOS as a flake input only after the standalone flow works.

## Working style

- Keep changes small and reviewable.
- Prefer explicit domain types over loosely typed JSON in the core.
- Add tests next to the behavior they protect.
- Document open questions in ADRs rather than leaving them implicit.
- Do not claim a feature works until it has been exercised end to end.
