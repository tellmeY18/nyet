# nyet

nyet is a headless NixOS configuration engine with multiple frontends.

It exposes NixOS options and current values, accepts pending changes, produces a reviewable plan, applies the plan through a narrow privileged helper, and supports rollback. The engine is independent of any one UI toolkit. CLI, Qt/QML, GTK4, web, and TUI clients can all speak the same protocol.

## Status

M1 (read model) and M2 (intents and plans) work against a fixture configuration. M3 (privileged apply and rollback) has an MVP implementation: a polkit-backed helper validates a plan file, writes the managed module and wrapper flake, builds as the requesting user, then runs a fixed `nh os switch` as root, records transactions, and rolls back one generation.

The apply path is fixed to `/etc/nixos` and requires the nyet NixOS module. Apply and rollback are verified by a disposable NixOS VM test in CI; the test covers the engine, helper, managed module, and one-step rollback.

The next milestone is first-run setup.

## Try it

```bash
nix develop

cargo run -p nyet-cli -- --flake tests/fixtures/basic --host basic options --prefix services.openssh
cargo run -p nyet-cli -- --flake tests/fixtures/basic --host basic get services.openssh.enable
cargo run -p nyet-cli -- --flake tests/fixtures/basic --host basic set services.nginx.enable true
cargo run -p nyet-cli -- --flake tests/fixtures/basic --host basic add boot.kernelModules loop
cargo run -p nyet-cli -- --flake tests/fixtures/basic --host basic intents
cargo run -p nyet-cli -- --flake tests/fixtures/basic --host basic plan --check
```

`nyet plan` prints the module and diff without writing anything. `--check` evaluates the candidate module and prints the NixOS toplevel derivation path. Use `--json` for machine-readable output and `--base PATH` to diff against an existing module.

On a NixOS host with the nyet module enabled, the write path is:

```bash
nyet --flake /etc/nixos --host "$(hostname)" set services.openssh.enable true
nyet --flake /etc/nixos --host "$(hostname)" plan --check
nyet --flake /etc/nixos --host "$(hostname)" apply
nyet history
nyet rollback
```

`apply` requires the flake at `/etc/nixos`, confirms in a terminal unless `--yes` is given, validates the candidate module, stages a short-lived plan under the user runtime directory, and invokes `nyet-helper` through `pkexec`. `apply` and `rollback` do not support `--json`; use `nyet history --json`.

Enable the helper and polkit rule from the flake:

```nix
{
  inputs.nyet.url = "path:/path/to/nyet";
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

`nyet.nixosModules.default` installs both binaries and adds a polkit rule requiring admin authentication for the helper.

Pending intents live in the nyet state directory (`$XDG_STATE_HOME/nyet` or `~/.local/state/nyet`). The option cache lives in the cache directory (`$XDG_CACHE_HOME/nyet` or `~/.cache/nyet`). Override them with `NYET_STATE_DIR` and `NYET_CACHE_DIR`; use `--no-cache` or `--refresh` to bypass or refresh the cache.

Current limits:

- Bulk defaults use `defaultText`; a typed default is fetched only for simple scalar options.
- List additions render nyet-owned members and merge with user lists. Removing a member that another module defines is not supported.
- The check evaluates a derivation path; it does not build or dry-build a NixOS system.
- Apply only supports a flake at `/etc/nixos` and a host name made of letters, digits, `_`, or `-`.
- Apply rejects option paths that require quoting, such as `boot.kernel.sysctl."kernel.hostname"`.
- The managed module must use nyet's restricted literal grammar; arbitrary Nix expressions are rejected by the helper.
- Rollback is one step (`nh os rollback`) and restores the previous managed module, not an arbitrary generation.
- There is no first-run setup, protocol, daemon, or GUI.

See [docs/roadmap.md](docs/roadmap.md).

## Why

NixOS has a large, typed configuration surface, but no stable, reusable, safe API for tools to build on. Existing editors tend to be desktop-only, unmaintained, or tightly coupled to one UI and one privilege path.

nyet separates the problem into three pieces:

- **Engine** — evaluates the NixOS module system, owns pending intents and plans, renders a managed module, and records transactions.
- **Frontends** — present the option model and collect intent; they never invoke Nix or root operations directly.
- **Helper** — a small polkit-backed binary that can write the managed module and run the rebuild, with fixed paths and fixed operations.

## Architecture

```text
CLI / Qt / GTK / Web / TUI
            |
      JSON-RPC over Unix socket
            |
       nyet engine
       /          \
  Nix evaluation   transaction store
            |
     nyet-helper (polkit)
            |
   managed module + nh
```

This is the target shape. Today the CLI calls `nyet-core` directly; the socket and `nyet-protocol` crate are added when a second client exists.

Core rules:

- NixOS evaluation is the source of truth. nyet does not parse Nix to discover effective configuration.
- nyet writes a managed module; it does not rewrite the user's hand-written configuration.
- Changes are expressed as intents, then a plan, then an applied transaction.
- The helper never accepts arbitrary file paths or arbitrary configuration content from a frontend.
- NixOS configuration, not a nyet database, remains authoritative.

### Crates

Dependency rule: `nyet-cli -> nyet-core`; `nyet-helper -> nothing`.

| Crate | Responsibility |
|---|---|
| `nyet-core` | The only component that understands NixOS configuration: option index, evaluation, values, intents, validation, plans, diffs, managed-module rendering, transactions. |
| `nyet-cli` | Reference client (`nyet`): subcommands, output, exit codes. Contains no configuration logic and evaluates no Nix. |
| `nyet-helper` | Privileged executor (`nyet-helper`): validate a plan id, write the managed module at a fixed path, run a fixed rebuild, record generations, roll back. No Nix evaluation, no client-supplied paths or content. |

The client contract is intent-only: clients read the model and express desires on options. They cannot request imperative operations. Package changes are list-membership intents, not an `install` command.

The wire layer is deferred. Until a second client exists, the CLI calls `nyet-core` directly; the socket and `nyet-protocol` crate are added then.

More detail: [docs/architecture.md](docs/architecture.md), [docs/protocol.md](docs/protocol.md), [docs/nixos-integration.md](docs/nixos-integration.md), [docs/testing.md](docs/testing.md).

## Repository layout

```text
crates/
  nyet-core/      domain model, option index, plans, transactions
  nyet-cli/       headless command-line frontend
  nyet-helper/    narrow privileged helper
tests/
  fixtures/       fixture flakes and NixOS configurations
docs/
  decisions/      architecture decision records
```

`nyet-protocol` is added only when a second client exists. Frontends will live under a top-level `frontends/` directory once the engine API is stable enough to consume.

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

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
