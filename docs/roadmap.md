# Roadmap

Status: draft. M0, the POC vertical slice, M1, M2, and the M3 managed-module MVP are complete. M4 has repository scan, import/error indexing, and standard-layout detection. M5 has classic migration planning, apply, snapshots, and exact rollback. Definition indexing and candidate evaluation are next.

The order is intentional. Do not build graphical clients before the repository model, migration, and edit paths are proven through the CLI. Tests are part of done for every milestone.

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

## M1: Read-only option model — done

- Harden the POC read path.
- Produce the full option index with types, defaults, descriptions, and internal flags.
- Read current values and basic provenance for selected paths.
- Cache by flake contents, host, and schema version.
- CLI commands: `nyet options`, `nyet get`.

## M2: Intents and plans — done

- Define the complete typed value model.
- Store pending intents, including list membership.
- Validate intents against option metadata.
- Render a candidate managed module deterministically.
- Produce a diff and candidate-module evaluation check.
- CLI commands: `nyet set`, `nyet unset`, `nyet add`, `nyet remove`, `nyet intents`, `nyet plan`, `nyet plan --check`.

The evaluation check is not a NixOS dry-build. A real dry-build on the target platform belongs with apply and the VM tests.

## M3: Privileged apply and rollback — MVP done

- Implement the helper with a validated plan spool.
- Write the managed module and wrapper flake atomically.
- Build as the requesting user, activate as root with `nh`.
- Record transactions and generations.
- Implement one-step rollback.
- CLI commands: `nyet apply`, `nyet rollback`, `nyet history`.
- Ship `packages.default` and `nixosModules.default` with a polkit rule.

Implemented: plan staging, helper validation, fixed `/etc/nixos` input, wrapper and managed module, exclusive lock, user build plus root switch, transaction records, and one-step rollback.

A disposable NixOS VM test applies a hostname change, rolls it back, and applies again through a non-root plan to exercise the user build path. GitHub Actions runs the VM test. The managed-module model is superseded by M4–M6.

## M4: Repository index — in progress

- Discover the configuration root and host.
- Parse every `.nix` file with a lossless CST parser.
- Index files, imports, definitions, package lists, and edit ranges.
- Correlate evaluated provenance with CST definitions.
- Mark ambiguous or dynamic definitions uneditable.
- Add `nyet scan` and machine-readable output.
- Cache by repository fingerprint, host, and schema version.

Done so far: file walk, relative import collection, syntax errors, configuration kind, host argument, standard-layout detection, and cached option evaluation.

Done when a fixture repository, including one that does not use the standard layout, can be scanned without mutation, and every definition site is either editable or explicitly rejected.

## M5: Standard layout and migration — in progress

- Define the canonical layout.
- Detect flake, classic, and mixed repositories.
- Build a structural migration plan.
- Snapshot files and evaluate both old and candidate configurations.
- Show the complete diff and require confirmation.
- Apply atomically; record an exact rollback snapshot.
- Add `nyet init`.

Done so far: classic-to-standard plan, source hashes, helper apply, snapshot under `/var/lib/nyet/migrations`, transaction record, exact migration rollback, and `nyet init`. Candidate evaluation, unified diffs, and nonstandard-flake migration remain.

Done when a classic configuration and an unnormalized flake can be migrated without semantic changes, idempotently, with exact rollback on failure.

## M6: In-place edits, packages, and tiers

- Edit existing definitions with CST-preserving changes.
- Add missing definitions to a selected file.
- Handle multiple definitions and ambiguity.
- Add and remove package-list members.
- Add `basic`, `intermediate`, and `advanced` tier metadata.
- Evaluate every candidate and reject stale or conflicting plans.
- Add CLI edit, package, and tier flows.

Done when a user can install and remove a package, change a supported option, inspect the Nix diff, apply it, and roll it back without a managed module.

## M7: Engine service, protocol, and first software store

- Add `nyet-protocol` with wire types and JSON-RPC methods.
- Add the unprivileged engine service and socket.
- Add repository scan, migration, option, package, plan, transaction, and event methods.
- Add the first software-store client.
- Keep the CLI as the reference client.

Done when a software store can search packages, install and remove them, configure basic application options during installation, and apply or roll back without evaluating Nix itself.

## M8: Additional clients

- Qt/QML.
- GTK4/libadwaita.
- Web.
- TUI.

Only start these after the protocol has survived the CLI and the first store.

## M9: SnowflakeOS and ztOS integration

- Consume nyet as a flake input.
- Respect each distribution's repository conventions.
- Add a Settings entry point.
- Reuse the engine and helper.
- Keep frontend-specific design in the distribution.

Done when the distribution can inspect and change a supported NixOS setting through nyet without embedding engine logic in the shell.

## Cross-cutting work

- Extensive automated suites: unit tests next to code, fixture-backed repository and evaluation tests, golden edit and migration tests, protocol compatibility tests, and gated Linux NixOS VM tests.
- Security review of the helper, relative-path validation, snapshots, and migration.
- Conflict and provenance UX.
- Curated option tiers and beginner-first defaults.
- Package metadata and software-store presentation.
- Assistant proposal validation, privacy, and prompt-injection defenses.
- Documentation and examples.
- Packaging for NixOS and SnowflakeOS.

See [testing.md](testing.md).

## Explicitly deferred

- Home Manager and nix-darwin backends.
- Multi-host orchestration.
- Remote management over the network.
- A general-purpose Nix expression UI.
- Replacing NixOS with a nyet-specific configuration model.
