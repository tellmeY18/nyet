# nyet

nyet is a headless, repository-aware NixOS configuration engine with multiple frontends.

It scans an existing NixOS repository, indexes its files and definitions, evaluates the full option surface, and turns software-store or settings-application actions into reviewable declarative Nix. The engine is independent of any one UI toolkit. CLI, Qt/QML, GTK4, web, and TUI clients can all speak the same protocol.

Latest alpha: [`v0.1.0-alpha.1`](https://github.com/tellmeY18/nyet/releases/tag/v0.1.0-alpha.1).

## Status

The current implementation is the M1 read model, the M2 intents and plans model, the managed-module MVP for M3, and the classic migration slice for M4/M5. It scans `.nix` files with `rnix`, renders a classic-to-standard plan, and applies it through the helper with a snapshot and exact rollback. It does not evaluate or activate the migrated configuration. Apply, rollback, and migration are verified by a disposable NixOS VM test in CI.

The target architecture is different and more ambitious:

- Scan and parse the entire configuration repository using a lossless CST parser.
- Correlate evaluated options with their source definitions.
- Normalize the repository into one supported layout during explicit first-run onboarding.
- Edit existing definitions in place, including package lists.
- Serve software stores through a long-running engine and JSON-RPC protocol.
- Present options through `basic`, `intermediate`, and `advanced` tiers.

Repository scan, classic migration planning, migration apply, snapshots, and exact rollback are implemented. Candidate evaluation, definition-level provenance, in-place editing, package-store flows, the protocol, daemon, and GUI are not. See [ADR 0005](docs/decisions/0005-repo-aware-editing-and-migration.md) and [ADR 0006](docs/decisions/0006-beginner-first-clients.md).

> **Alpha warning:** `v0.1.0-alpha.1` is for testing on a disposable or fully
> backed-up NixOS machine. Migration is classic-only and does not build or
> activate the migrated configuration. Review the plan and generated
> `flake.nix` before switching.

## Install the alpha

Add the pinned flake input and enable the module:

```nix
{
  inputs.nyet.url = "github:tellmeY18/nyet/v0.1.0-alpha.1";
  outputs = { nixpkgs, nyet, ... }: {
    nixosConfigurations.host = nixpkgs.lib.nixosSystem {
      modules = [
        nyet.nixosModules.default
        { programs.nyet.enable = true; }
      ];
    };
  };
}
```

Rebuild once to install `nyet`, `nyet-helper`, and the polkit rule. Then review
and run the migration as root:

```bash
sudo cp -a /etc/nixos /etc/nixos.pre-nyet
sudo nyet --flake /etc/nixos --host "$(hostname)" migration-plan --system x86_64-linux
sudo nyet --flake /etc/nixos --host "$(hostname)" init --system x86_64-linux
nixos-rebuild switch --flake /etc/nixos#"$(hostname)"
```

The migration snapshots the original files and can be reverted with
`nyet migration-rollback <snapshot-id> --yes`. Keep the manual backup until the
new generation boots successfully.

## Try it

The commands below exercise the managed-module MVP, repository scan, and classic migration. They do not exercise the target in-place editing model.

```bash
nix develop

cargo run -p nyet-cli -- --flake tests/fixtures/basic --host basic options --prefix services.openssh
cargo run -p nyet-cli -- --flake tests/fixtures/basic --host basic get services.openssh.enable
cargo run -p nyet-cli -- --flake tests/fixtures/basic --host basic set services.nginx.enable true
cargo run -p nyet-cli -- --flake tests/fixtures/basic --host basic add boot.kernelModules loop
cargo run -p nyet-cli -- --flake tests/fixtures/basic --host basic intents
cargo run -p nyet-cli -- --flake tests/fixtures/basic --host basic plan --check
cargo run -p nyet-cli -- --flake /path/to/classic --host host scan --json
cargo run -p nyet-cli -- --flake /path/to/classic --host host migration-plan --system x86_64-linux
cargo run -p nyet-cli -- --flake /etc/nixos --host host init --system x86_64-linux
cargo run -p nyet-cli -- migration-rollback <snapshot-id> --yes
```

`nyet plan` prints the module and diff without writing anything. `--check` evaluates the candidate module and prints the NixOS toplevel derivation path. Use `--json` for machine-readable output and `--base PATH` to diff against an existing module.

On a NixOS host with the nyet module enabled, the current write path is:

```bash
nyet --flake /etc/nixos --host "$(hostname)" set services.openssh.enable true
nyet --flake /etc/nixos --host "$(hostname)" plan --check
nyet --flake /etc/nixos --host "$(hostname)" apply
nyet history
nyet rollback
```

`apply` requires the flake at `/etc/nixos`, confirms in a terminal unless `--yes` is given, validates the candidate module, stages a short-lived plan under the user runtime directory, and invokes `nyet-helper` through `pkexec`. `apply` and `rollback` do not support `--json`; use `nyet history --json`.

For a classic `/etc/nixos`, `nyet init` previews the same migration plan, snapshots the original files, moves `configuration.nix` and `hardware-configuration.nix` under `hosts/<host>/`, generates `flake.nix`, and records a transaction. It does not build or switch; run `nixos-rebuild switch --flake /etc/nixos#<host>` when ready. `nyet migration-rollback <id>` restores the exact pre-migration files and refuses if a migrated file changed.

For local development, point `inputs.nyet.url` at `path:/path/to/nyet` instead
of the release tag. `nyet.nixosModules.default` installs both binaries and adds
a polkit rule requiring admin authentication for the helper.

Pending intents live in the nyet state directory (`$XDG_STATE_HOME/nyet` or `~/.local/state/nyet`). The option cache lives in the cache directory (`$XDG_CACHE_HOME/nyet` or `~/.cache/nyet`). Override them with `NYET_STATE_DIR` and `NYET_CACHE_DIR`; use `--no-cache` or `--refresh` to bypass or refresh the cache.

Current limits:

- Bulk defaults use `defaultText`; a typed default is fetched only for simple scalar options.
- List additions render nyet-owned members and merge with user lists. Removing a member that another module defines is not supported.
- The check evaluates a derivation path; it does not build or dry-build a NixOS system.
- Apply only supports a flake at `/etc/nixos` and a host name made of letters, digits, `_`, or `-`.
- Apply rejects option paths that require quoting, such as `boot.kernel.sysctl."kernel.hostname"`.
- The managed module must use nyet's restricted literal grammar; arbitrary Nix expressions are rejected by the helper.
- Rollback is one step (`nh os rollback`) and restores the previous managed module, not an arbitrary generation.
- Migration supports classic `/etc/nixos` only. Nonstandard flakes and relative imports that migration cannot relocate are refused.
- Migration writes a flake from scratch. It uses a local nixpkgs channel when one is found and otherwise falls back to `github:NixOS/nixpkgs/nixos-unstable`; review the input before switching.
- Migration does not build, evaluate, or activate the candidate configuration, and it does not preserve a `flake.lock` from a previous layout because classic repositories do not have one.
- Definition-level provenance and in-place editing are not implemented.
- There is no package store, protocol, daemon, or GUI.

See [docs/roadmap.md](docs/roadmap.md).

## Why

NixOS has a large, typed configuration surface, but no stable, reusable, safe API for tools to build on. Existing editors tend to be desktop-only, unmaintained, or tightly coupled to one UI and one privilege path. Beginners arriving from imperative systems are expected to learn Nix before they can install an application or change a setting.

nyet separates the problem into four pieces:

- **Repository index** — discovers the configuration root, parses files with a lossless CST, indexes definitions and edit ranges, and correlates them with evaluation.
- **Engine** — evaluates the NixOS module system, owns intents, plans, edits, migrations, and transactions, and serves clients over a local protocol.
- **Frontends** — software stores and settings applications present the model and collect intents; they never invoke Nix or root operations directly.
- **Helper** — a small polkit-backed binary that applies root-owned edits and runs the rebuild, with fixed roots, fixed operations, and validated relative paths.

The product is training wheels: every action becomes ordinary Nix in the user's repository, and the user can inspect, edit, or remove it without nyet.

## Architecture

```text
Software stores / CLI / Qt / GTK / Web / TUI
                    |
          JSON-RPC over Unix socket
                    |
              nyet engine
       /       |          \
 repository  Nix eval   transaction
   index      model        store
       \       |          /
          nyet-helper (polkit)
                    |
        repository edits + nh
```

This is the target shape. Today the CLI calls `nyet-core` directly and writes a managed module. Repository scan, classic migration apply, snapshots, and rollback exist; definition provenance, in-place editing, the socket, and the `nyet-protocol` crate are not implemented.

Core rules:

- NixOS evaluation is the source of truth for effective configuration.
- Parsing supplies structure, provenance, and edit locations only.
- nyet edits existing definitions in the user's repository; it does not hide settings in a nyet-only module.
- First-run migration normalizes structure, not semantics, and is explicit, diffed, evaluated, and reversible.
- Package changes are list-membership intents, never imperative installs.
- The helper never accepts arbitrary paths or configuration content from a frontend.
- NixOS configuration, not a nyet database, remains authoritative.

### Crates

Dependency rule: `nyet-cli -> nyet-core`; `nyet-helper -> nothing`; `nyet-protocol` depends only on wire types.

| Crate | Responsibility |
|---|---|
| `nyet-core` | The only component that understands NixOS configuration: repository index, CST parsing, evaluation, values, intents, validation, plans, edits, migration, diffs, transactions. |
| `nyet-cli` | Reference client (`nyet`): subcommands, output, exit codes. Contains no configuration logic and evaluates no Nix. |
| `nyet-helper` | Privileged executor (`nyet-helper`): validate a plan id, apply scoped edits under a fixed root, run a fixed rebuild, record generations, roll back. No Nix evaluation, no client-supplied paths or content. |
| `nyet-protocol` | Wire types and JSON-RPC schema only (planned). |

The client contract is intent-only: clients read the model and express desires on options, package lists, and edit targets. They cannot request imperative operations. Package changes are list-membership intents, not an `install` command.

More detail: [docs/architecture.md](docs/architecture.md), [docs/protocol.md](docs/protocol.md), [docs/nixos-integration.md](docs/nixos-integration.md), [docs/testing.md](docs/testing.md).

## Repository layout

```text
crates/
  nyet-core/      repository index, evaluation, plans, edits, transactions
  nyet-cli/       headless command-line frontend
  nyet-helper/    narrow privileged helper
  nyet-protocol/  JSON-RPC types (planned)
tests/
  fixtures/       fixture flakes and NixOS configurations
  golden/         expected edits, migrations, and diffs
docs/
  decisions/      architecture decision records
```

After first-run migration, a managed NixOS repository uses:

```text
flake.nix
hosts/<host>/configuration.nix
hosts/<host>/hardware-configuration.nix
modules/...
```

Frontends will live under a top-level `frontends/` directory once the engine API is stable enough to consume.

## Development

```bash
nix develop
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Do not use `nix profile` for development. Use the flake dev shell.

Tests are part of done. Unit tests live next to the code; Nix-backed tests use fixture configurations under `tests/fixtures/` and never the developer's host. See [docs/testing.md](docs/testing.md).

## Documentation

- [Architecture](docs/architecture.md)
- [Protocol](docs/protocol.md)
- [NixOS integration](docs/nixos-integration.md)
- [Testing](docs/testing.md)
- [Roadmap](docs/roadmap.md)
- [Prior art](docs/prior-art.md)
- [ADR 0001: engine and frontends](docs/decisions/0001-engine-and-frontends.md)
- [ADR 0002: crate boundaries and intent-only client](docs/decisions/0002-crate-boundaries.md)
- [ADR 0003: option evaluation](docs/decisions/0003-option-evaluation.md)
- [ADR 0004: apply handoff](docs/decisions/0004-apply-handoff.md)
- [ADR 0005: repository-aware editing and migration](docs/decisions/0005-repo-aware-editing-and-migration.md)
- [ADR 0006: beginner-first software-store clients](docs/decisions/0006-beginner-first-clients.md)

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
