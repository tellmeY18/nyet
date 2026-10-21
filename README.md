# nyet

nyet is a headless NixOS configuration engine with multiple frontends.

It exposes NixOS options and current values, accepts pending changes, produces a reviewable plan, applies the plan through a narrow privileged helper, and supports rollback. The engine is independent of any one UI toolkit. CLI, Qt/QML, GTK4, web, and TUI clients can all speak the same protocol.

## Status

Early architecture and scaffold. The repository currently contains the project context, protocol draft, roadmap, and a minimal Rust workspace. No functional engine or frontend exists yet.

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
   managed module + nixos-rebuild
```

Core rules:

- NixOS evaluation is the source of truth. nyet does not parse Nix to discover effective configuration.
- nyet writes a managed module; it does not rewrite the user's hand-written configuration.
- Changes are expressed as intents, then a plan, then an applied transaction.
- The helper never accepts arbitrary file paths or arbitrary configuration content from a frontend.
- NixOS configuration, not a nyet database, remains authoritative.

More detail: [docs/architecture.md](docs/architecture.md), [docs/protocol.md](docs/protocol.md), [docs/nixos-integration.md](docs/nixos-integration.md).

## Repository layout

```text
crates/
  nyet-core/      domain model, option index, plans, transactions
  nyet-cli/       headless command-line frontend
  nyet-helper/    narrow privileged helper
docs/
  decisions/      architecture decision records
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

## Documentation

- [Architecture](docs/architecture.md)
- [Protocol](docs/protocol.md)
- [NixOS integration](docs/nixos-integration.md)
- [Roadmap](docs/roadmap.md)
- [Prior art](docs/prior-art.md)
- [ADR 0001: engine and frontends](docs/decisions/0001-engine-and-frontends.md)

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
