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

## Design changes

- Keep the engine independent of UI toolkits.
- Keep NixOS evaluation as the source of truth.
- Do not broaden the helper without a security review.
- Record architectural changes in `docs/decisions/`.

## Security

- Never commit secrets, host-specific keys, or private configuration.
- Do not pass configuration content or arbitrary paths through the privileged helper.
- Treat frontends as untrusted from the helper's perspective.

## License

Contributions are accepted under GPL-3.0-or-later. See [LICENSE](LICENSE).
