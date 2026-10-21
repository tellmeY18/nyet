# Roadmap

Status: draft.

The order is intentional. Do not build a graphical frontend before the read and plan paths are proven through the CLI.

## M0: Scaffold

- Repository, license, context documents.
- Rust workspace with engine, CLI, and helper placeholders.
- Nix dev shell.

Done when the repository builds and the context is sufficient for a new contributor or agent.

## M1: Read-only option model

- Evaluate a real NixOS configuration.
- Produce an option index with types, defaults, and descriptions.
- Read current values and basic provenance for selected paths.
- Cache by configuration and nixpkgs revision.
- CLI commands: `nyet options`, `nyet get`, `nyet eval`.

Done when the CLI can inspect a real host without writing anything.

## M2: Intents and plans

- Define the typed value model.
- Store pending intents.
- Validate intents against option metadata.
- Render a candidate managed module.
- Produce a diff and dry-build result.
- CLI commands: `nyet set`, `nyet plan`, `nyet plan show`.

Done when a user can prepare a change and inspect the exact module and diff without applying it.

## M3: Privileged apply and rollback

- Implement the helper with a validated plan spool.
- Write the managed module atomically.
- Run a fixed rebuild operation.
- Record transactions and generations.
- Implement rollback.
- CLI commands: `nyet apply`, `nyet rollback`, `nyet history`.

Done when a disposable NixOS VM can apply a change, verify it, and roll it back.

## M4: First-run setup

- Detect flake and classic configurations.
- Detect the host attribute.
- Create and import the managed module with explicit confirmation.
- Record setup as a transaction.

Done when a fresh NixOS VM can be onboarded without manual file editing beyond the confirmed import.

## M5: Qt/QML frontend

- Consume the engine protocol.
- Read-only option browser first.
- Typed editors for supported values.
- Plan review and apply progress.
- Rollback and history.

Done when the frontend can complete the same flow as the CLI without direct Nix or root access.

## M6: Additional frontends

- GTK4/libadwaita.
- Web.
- TUI.

Only start these after the protocol has survived the CLI and Qt clients.

## M7: ztOS integration

- Consume nyet as a flake input.
- Add a Settings entry point.
- Reuse the engine and helper.
- Keep frontend-specific design in ztOS.

Done when ztOS can inspect and change a supported NixOS setting through nyet without embedding engine logic in the shell.

## Cross-cutting work

- NixOS VM integration tests.
- Protocol compatibility tests.
- Security review of the helper.
- Conflict and provenance UX.
- Documentation and examples.
- Packaging for NixOS.

## Explicitly deferred

- Managing arbitrary Nix expressions as first-class UI.
- Home Manager and nix-darwin backends.
- Multi-host orchestration.
- Remote management over the network.
- Package store features.
