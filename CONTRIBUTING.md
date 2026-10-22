# Contributing

Read [AGENTS.md](AGENTS.md) and the documents in [docs/](docs/) before changing architecture or the privileged path.

## Development

```bash
nix develop
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The primary development host may be macOS. NixOS VM integration tests require Linux or CI.

## Testing

- Every behavior gets tests with the change that introduces it.
- Unit tests live next to the code.
- Nix-backed tests run against fixture configurations under `tests/fixtures/`, never the developer's host.
- Rendered modules and plan diffs are protected by golden tests.
- Default tests require no root and do not mutate a real configuration.
- Linux-only NixOS VM tests are gated and run in CI.

See [docs/testing.md](docs/testing.md).

## Design changes

- Keep the engine independent of UI toolkits.
- Keep NixOS evaluation as the source of truth.
- Keep the dependency rule: `nyet-cli -> nyet-core`; `nyet-helper -> nothing`. Do not add a shared utility crate.
- Clients express intents only; do not add imperative operations.
- Do not broaden the helper without a security review.
- Record architectural changes in `docs/decisions/`.

## Security

- Never commit secrets, host-specific keys, or private configuration.
- Do not pass configuration content or arbitrary paths through the privileged helper.
- Treat frontends as untrusted from the helper's perspective.

## License

Contributions are accepted under GPL-3.0-or-later. See [LICENSE](LICENSE).
