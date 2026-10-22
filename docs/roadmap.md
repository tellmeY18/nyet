# Roadmap

Status: draft. M0, the POC vertical slice, M1, and M2 are complete. M3 has an MVP implementation; its VM gate is still open.

The order is intentional. Do not build a graphical frontend before the read and plan paths are proven through the CLI. Tests are part of done for every milestone.

## M0: Scaffold — done

- Repository, license, context documents.
- Crate contracts and the intent-only client model.
- Rust workspace with engine, CLI, and helper placeholders.
- Nix dev shell.

Done when the repository builds and the context is sufficient for a new contributor or agent.

## POC: Vertical slice — done

Proved the read and plan loop on a fixture with no privilege and no host writes.

- Evaluate a fixture flake's NixOS configuration.
- Build an option index for that configuration.
- CLI: `nyet options`, `nyet get <path>`.
- Accept one set intent.
- Render the managed module and show the diff.
- Write nothing, run no build, use no helper.

Done when the fixture slice runs end to end and is covered by unit, fixture-evaluation, and golden-render tests. This intentionally cuts across parts of M1 and M2: no cache, no daemon, no protocol, no privilege.

## M1: Read-only option model — done

- Harden the POC read path.
- Produce the full option index with types, defaults, descriptions, and internal flags.
- Read current values and basic provenance for selected paths.
- Cache by flake contents, host, and schema version.
- CLI commands: `nyet options`, `nyet get`.

Done when the CLI can inspect a real host without writing anything, with fixture tests for every read behavior.

## M2: Intents and plans — done

- Define the complete typed value model.
- Store pending intents, including list membership.
- Validate intents against option metadata.
- Render a candidate managed module deterministically.
- Produce a diff and candidate-module evaluation check.
- CLI commands: `nyet set`, `nyet unset`, `nyet add`, `nyet remove`, `nyet intents`, `nyet plan`, `nyet plan --check`.

Done when a user can prepare a change and inspect the exact module and diff without applying it, with golden tests for rendering and diffs.

The evaluation check is not a NixOS dry-build. A real dry-build on the target platform belongs with apply and the VM tests.

## M3: Privileged apply and rollback — MVP implemented, VM gate open

- Implement the helper with a validated plan spool.
- Write the managed module and wrapper flake atomically.
- Run a fixed rebuild operation.
- Record transactions and generations.
- Implement one-step rollback.
- CLI commands: `nyet apply`, `nyet rollback`, `nyet history`.
- Ship `packages.default` and `nixosModules.default` with a polkit rule.

Implemented: plan staging under `/run/user/<uid>/nyet/plans`, helper validation of owner/mode/size/expiry/host/literal grammar, fixed `/etc/nixos` input, wrapper at `/var/lib/nyet/flake.nix`, managed module at `/var/lib/nyet/nyet.nix`, an exclusive transaction lock, `nh os switch /var/lib/nyet#<host>`, transaction JSON records, and `nh os rollback`.

A disposable NixOS VM test (`checks.x86_64-linux.vm`) applies a hostname change and rolls it back; GitHub Actions runs it, and the gate is green.

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

- Extensive automated suites: unit tests next to code, fixture-backed evaluation tests, golden render and diff tests, protocol compatibility tests, and gated Linux NixOS VM tests.
- Security review of the helper.
- Conflict and provenance UX.
- Documentation and examples.
- Packaging for NixOS.

See [testing.md](testing.md).

## Explicitly deferred

- Managing arbitrary Nix expressions as first-class UI.
- Home Manager and nix-darwin backends.
- Multi-host orchestration.
- Remote management over the network.
- Package store features.
