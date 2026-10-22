# Changelog

All notable changes to nyet are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions use
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-alpha.1] - 2026-10-22

First public alpha. This release is intended for testing on disposable or
fully backed-up NixOS machines. It is not ready for unattended use.

### Added

- Repository scanning for classic and flake configurations with `rnix`.
- `nyet options`, `get`, `set`, `unset`, `add`, `remove`, `intents`, `plan`,
  `apply`, `rollback`, `history`, `scan`, `migration-plan`, and `init`.
- Typed option paths, values, validation, and a cached option index.
- Deterministic managed-module rendering, line diffs, and candidate evaluation
  with `nyet plan --check`.
- A scoped privileged helper with plan staging, fixed `/etc/nixos` input,
  atomic edits, transaction records, and one-step rollback.
- Classic `/etc/nixos` migration to:

  ```text
  flake.nix
  hosts/<host>/configuration.nix
  hosts/<host>/hardware-configuration.nix
  ```

- Migration source hashes, a snapshot under `/var/lib/nyet/migrations/<id>`,
  generated flake input handling, exact rollback, and refusal to roll back
  modified files.
- `packages.default`, `nixosModules.default`, and a polkit rule requiring admin
  authentication for `nyet-helper`.
- A disposable NixOS VM test covering migration, migrated-flake build,
  migration rollback, managed apply, and managed rollback.

### Known limits

- Classic `/etc/nixos` migration only; nonstandard flakes and relative imports
  that cannot be relocated are refused.
- Migration does not evaluate, build, or activate the candidate configuration.
  Review the generated `flake.nix`, then run `nixos-rebuild switch` manually.
- The generated flake uses a local nixpkgs channel when available and otherwise
  falls back to `github:NixOS/nixpkgs/nixos-unstable`.
- `nyet plan --check` evaluates a toplevel derivation path; it does not
  dry-build the system.
- Apply uses a managed module and supports only unquoted option paths.
- Definition-level provenance, in-place editing, package-list editing, tier
  metadata, the engine service, protocol, package store, and GUI are not
  implemented.
- Generation rollback is one step and restores the previous managed module,
  not an arbitrary generation.

### Security

- The helper accepts plan identifiers, never arbitrary paths or configuration
  content from a frontend.
- Root-owned repository edits are scoped to `/etc/nixos` with validated
  relative paths and source hashes.
- Migration snapshots are root-only. Candidate builds run as the requesting
  user from a private staging directory.

[0.1.0-alpha.1]: https://github.com/tellmeY18/nyet/releases/tag/v0.1.0-alpha.1
