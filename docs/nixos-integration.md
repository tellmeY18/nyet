# NixOS integration

Status: draft. The current code implements the managed-module MVP plus repository scan, classic migration planning, and classic migration apply with a snapshot and exact rollback. Candidate evaluation and in-place editing are not implemented.

## Source of truth

The user's NixOS repository is authoritative for configuration. NixOS evaluation is authoritative for effective values, types, defaults, and conflicts.

nyet owns:

- Repository discovery and indexing.
- Pending intents.
- Plans and proposed edits.
- Migration snapshots and transaction metadata.
- Build and activation state.

nyet does not own:

- The effective configuration.
- Nixpkgs.
- The Nix store.
- The active system generation.

## Configuration roots

The engine discovers one configuration root. It does not accept an arbitrary write path from a client.

Supported inputs:

- A flake-based repository with `nixosConfigurations.<host>`.
- A classic `configuration.nix` that can be migrated to a flake.

The default root on NixOS is `/etc/nixos`, but the engine must also work against a user-owned checkout or a SnowflakeOS-managed repository.

The engine needs:

- The configuration root.
- The host attribute name.
- Whether the root is user-owned or root-owned.
- The current system generation, when activating.

## Standard layout

After migration, the supported layout is:

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
- Additional modules may be imported explicitly.
- Existing definitions, package lists, comments, and imports are preserved.
- Read-only scanning works before migration.

This is an opinionated structure, not a requirement for reading an existing repository.

## Repository index

The engine parses every `.nix` file under the root with a lossless CST parser and records:

- File and import edges.
- Flake outputs and host attributes where statically visible.
- Definitions with option paths, byte ranges, and surrounding context.
- Package lists and their entries.
- Conditions such as `mkIf` and `mkMerge` where they can be identified.
- Whether a definition is safely editable.

Parsing is for structure, provenance, and edit locations. It never supplies effective values. A dynamic or ambiguous definition is marked uneditable.

The index is cached by:

- Canonical root path.
- Repository fingerprint including `flake.lock` and `.nix` files.
- Host name.
- Engine schema version.

Cache failures never fail a read.

## Option discovery

Options must come from evaluation, not from parsing source files.

The engine should produce or consume an option index containing:

- Option path.
- Type.
- Description.
- Default.
- Example.
- Read-only flag.
- Declarations and definitions.
- Visibility and internal flags.
- UI tier metadata, when curated.

Current values and provenance are fetched for selected paths. Full provenance is not promised until it is measured on real repositories.

## Migration

First-run migration is explicit and reversible:

1. Discover the root, flake or classic entrypoint, host, and hardware configuration.
2. Verify nyet can evaluate the current configuration.
3. Snapshot every file that may change.
4. Build the candidate layout and import wiring.
5. Preserve existing option definitions, package lists, comments, and modules.
6. Evaluate the candidate configuration.
7. Show the complete file diff and warnings.
8. Require confirmation.
9. Apply atomically and record a migration snapshot.
10. Keep the snapshot until the user commits or rolls back.

Migration changes structure, not semantics. It must not rewrite arbitrary expressions to fit a template. If a safe structural migration cannot be produced, the engine reports the reason and leaves the repository unchanged.

The current classic slice implements steps 1, 3, 8, and 9 for `/etc/nixos`: it records source hashes, snapshots the moved files, writes `flake.nix`, and records a migration transaction. It refuses nonstandard flakes, relative imports it cannot relocate, and changed source files. Steps 2, 4, 6, 7, and 10 are not implemented: migration does not evaluate or build the candidate, does not print a unified diff, and does not activate it. Run `nixos-rebuild switch --flake /etc/nixos#<host>` after reviewing the generated flake. `nyet migration-rollback <id>` restores the snapshot and refuses if a migrated file changed.

`services.nyet.enable` may install and start the engine, but it must not migrate or rewrite configuration during activation. `nyet init` or a client onboarding flow initiates migration.

## Editing definitions

nyet edits existing definitions in the repository.

For an option with one editable definition:

- The plan selects that definition.
- The edit replaces or inserts the assignment in place.
- Comments, indentation, and unrelated attributes are preserved.

For an option with multiple definitions:

- The client may select a definition id from `config.definitions`.
- Without a selection, the engine proceeds only if the choice is unambiguous.
- Otherwise the plan fails as ambiguous and shows every definition.

For an option with no definition:

- The engine may add the assignment to the selected host file as part of a reviewed plan.
- It must not create a hidden nyet-only settings module.
- The user chooses the target file when more than one is valid.

For a reset:

- Remove the selected assignment so the NixOS default applies.
- If removing the assignment would change another contribution, report the conflict instead of guessing.

All edits are atomic, backed up, and part of a transaction. A stale before-hash rejects the plan and requires a rescan.

## Package lists

Package install and uninstall are list-membership intents.

- The default target is `environment.systemPackages`.
- The engine may support other configured package lists later.
- Install inserts a package reference into the selected list.
- Uninstall removes the selected member from the selected list.
- If the list does not exist, the engine may add it to the chosen host file in the same reviewed plan.
- Package search reads nixpkgs metadata; it does not install anything.
- `nix profile` and imperative package operations remain forbidden.

Package names are resolved against the evaluated `pkgs` set before a plan is accepted. A missing attribute is a structured error, not a partially applied edit.

## Evaluation

The engine shells out to `nix eval --json --impure` with embedded expressions that use `builtins.getFlake`.

- Effective values come from the evaluated configuration.
- The bulk index carries path, type, description, `defaultText`, flags, and declarations.
- Arbitrary defaults are not forced in bulk because some NixOS defaults throw.
- Values are sanitized before serialization: functions become null, derivations become their `outPath`, and paths become strings.
- Provenance is fetched for selected paths, not the bulk index.
- The CST index supplies edit ranges; evaluation supplies semantics.

See [ADR 0003](decisions/0003-option-evaluation.md) and [ADR 0005](decisions/0005-repo-aware-editing-and-migration.md).

## Build and apply

The current MVP stages a plan under `/run/user/<uid>/nyet/plans` (mode `0600`, ten-minute lifetime; `/run/nyet/plans/0` as root) and passes only the plan id to the helper through `pkexec`.

The target edit/apply flow adds file operations to the plan:

- Repository-relative edit paths.
- Before-hashes.
- Replacement text.
- Backups and a transaction snapshot.

The helper validates:

- The id is a 32-character lowercase hex string.
- The plan file is regular, owned by `PKEXEC_UID`, mode `0600` or stricter, and no larger than the size limit.
- The plan version, id, uid, host, creation time, expiry, and kind.
- Every edit path is relative, canonicalized under the fixed root, and not a symlink escape.
- Every before-hash matches the current file.
- The resulting file remains valid Nix and the candidate configuration evaluates.

For a user-owned repository, the engine may perform the file edits directly. For a root-owned repository, the helper performs them. In both cases the helper performs activation.

The helper builds as the requesting user with `/run/current-system/sw/bin/nh os build`, then runs `nh os switch` as root with a fixed environment. On failure it restores changed files and leaves the active generation unchanged.

Do not pass file content through a command-line argument. Use the validated plan spool.

## Managed-module MVP transition

The current implementation still uses:

- `/var/lib/nyet/nyet.nix` for generated settings.
- `/var/lib/nyet/flake.nix` as a wrapper around `/etc/nixos`.
- `/var/lib/nyet/nyet.nix.previous` for rollback.

This model is superseded by repository editing and migration. It remains in place for in-place option edits until the repository editor can replace it without regressing apply and rollback. Classic migration already rewrites `/etc/nixos` directly.

## Rollback

Rollback must restore both configuration and generation state:

- Record the generation before and after each apply.
- Record every changed file and its backup.
- On failure, leave the active generation unchanged and restore the files.
- On explicit rollback, run `nh os rollback`, restore the matching file snapshot, and record a new transaction.
- Migration rollback restores the exact pre-migration files.

The helper serializes apply, migration, and rollback with an exclusive lock on `/var/lib/nyet/lock`. Arbitrary recorded-generation rollback and build-log capture are not implemented.

Do not reconstruct a previous configuration by replaying intents.

## First-run setup

Target flow:

1. `services.nyet.enable = true` installs the engine, helper, polkit rule, and service.
2. The engine starts and scans the discovered repository.
3. The client shows the detected root, host, layout, and migration requirement.
4. If migration is needed, the engine produces a complete plan and diff.
5. The user confirms.
6. Migration applies atomically and records a snapshot.
7. The engine reindexes and reports the editable option surface.
8. Normal editing and package-store flows become available.

A failed migration leaves the repository exactly as it was.

## Testing

Repository scanning, CST edits, and migration output are tested against fixture repositories under `tests/fixtures/`, never a real host. Apply and rollback are verified in a disposable Linux VM and gated out of default test runs.

See [testing.md](testing.md).

## Future targets

Potential later backends:

- Home Manager.
- nix-darwin.
- system-manager.
- Multi-host flake configurations.

Keep the core model backend-neutral enough to add these without redesigning the protocol.

## Open questions

- How to evaluate options efficiently across NixOS versions.
- How to detect and present conflicting definitions.
- How to handle definitions generated by `mkIf` and `mkMerge`.
- How to make migration safe for unusual but valid classic configurations.
- How to present and safely support arbitrary expressions beyond supported values.
- How to make rollback target an arbitrary recorded generation.
- How to represent secrets and sensitive values without logging them.
- How to integrate with SnowflakeOS repository conventions.
