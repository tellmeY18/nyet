# ADR 0005: Repository-aware editing and opinionated migration

- Status: accepted
- Date: 2026-10-22
- Supersedes: ADR 0004 for the target write model
- Amends: ADR 0001, ADR 0002, ADR 0003

## Context

The MVP treats a generated module as the only write target. That is safe, but it is not the product the primary audience needs. A user arriving from an imperative desktop should be able to install software and change settings without first learning Nix. The changes must still be real Nix in the user's configuration, not opaque state in a nyet database.

The user also wants nyet to work on an existing repository without requiring a manual import or manual rewrite. On first use, nyet should understand the repository, normalize it to a supported layout, and then edit the definitions that already exist.

The repository is therefore a first-class input and output of the engine.

## Decision

### The repository is the configuration source

- A configuration root contains a flake-based NixOS configuration or a classic configuration that can be migrated to one.
- nyet scans every `.nix` file under the root, follows imports where possible, and builds a map of files, modules, definitions, and edit ranges.
- nyet edits existing definitions in the repository. It does not hide user settings in a nyet-only module.
- If an option has no definition, nyet may create one only in a chosen target file and only as part of a reviewed plan.

### Evaluation remains the semantic source of truth

- Effective values, types, defaults, conflicts, and warnings come from NixOS evaluation.
- Parsing is used for structure, provenance, and edit locations. It is never used to infer effective configuration.
- nyet uses an existing lossless Nix parser (`rnix`/`rowan` or an equivalent CST library). nyet does not implement a general Nix parser.
- When a definition is dynamic or ambiguous, nyet refuses to edit it rather than guessing.

### One supported layout

After migration, a repository uses a single, predictable layout:

```text
flake.nix
hosts/<host>/configuration.nix
hosts/<host>/hardware.nix
modules/...
```

Additional files may be imported by the host configuration. The layout is opinionated so every client sees the same structure, host boundary, and edit targets.

Read-only scanning must still work for repositories that have not been migrated.

### Migration is an explicit transaction

First use may migrate the repository into the supported layout:

1. Detect the configuration root, flake or classic entrypoint, host, and hardware configuration.
2. Snapshot every file that may change.
3. Generate or normalize the flake and host entrypoints.
4. Preserve existing option definitions, package lists, comments, and imported modules.
5. Produce a complete diff and evaluate the old and candidate configurations.
6. Apply atomically only after confirmation.
7. Record a migration transaction that can restore the exact original files.

Migration normalizes structure, not semantics. It must not rewrite user expressions merely to fit a template. Enabling `services.nyet.enable` may start the engine, but it must not silently rewrite configuration during activation. Migration is initiated by `nyet init` or by a client onboarding flow with explicit confirmation.

### Package operations remain declarative

- Install and uninstall are intents on a package list, normally `environment.systemPackages`.
- `nix profile`, imperative installs, and shell-out operations remain forbidden.
- Package search may read nixpkgs metadata, but applying a package change edits the configuration list.
- The engine may add the list or the target definition when it is absent, in the chosen file, with a visible diff.

### Privilege is split by ownership

- A user-owned configuration root is edited by the unprivileged engine.
- A root-owned configuration root is edited only through the helper, using a plan that names a fixed configuration root and relative file operations.
- The helper never accepts a path or whole configuration from a frontend. It validates every relative path, rejects traversal and symlink escapes, checks before-hashes, writes atomically, and keeps backups.
- Activation and rollback remain helper operations.

### Clients and protocol

Software stores are the primary clients. nyet therefore needs a long-running engine service and a stable protocol, rather than only the CLI calling the library.

- The engine is exposed as a local JSON-RPC service over a Unix socket.
- `services.nyet.enable` installs the helper and starts the engine for the user session.
- The CLI remains the reference client and debugging surface.
- Repository scan, migration, metadata, packages, plans, transactions, and events are protocol operations.
- Clients never send filesystem paths for writes and never send whole configurations.

### Assistant proposals are untrusted

An optional LLM or assistant may propose changes, but it may only produce intents or edit proposals. The engine validates, plans, diffs, and requires confirmation before applying. An assistant never writes files directly and never becomes a source of truth.

## Consequences

Positive:

- The user's configuration remains the single declarative source of truth.
- Every change is visible as a normal Nix diff and can be reviewed, copied, or hand-edited.
- Beginners get a software-store flow without learning Nix first.
- The supported layout makes provenance and edit targets predictable.
- The engine can graduate users from generated UI actions to manual configuration.

Negative:

- nyet now owns a parser-backed repository model, migration, and structural editing. This is substantially more complex than rendering one module.
- Migration can break a working configuration if evaluation is incomplete; snapshots and rollback are mandatory.
- Root-owned repositories need a more capable privileged edit path and a new security review.
- The protocol and service are no longer deferrable.
- Repository mutations need locking, conflict detection, and transactional file writes.

## Alternatives considered

### Keep the managed-module-only model

Rejected. It makes nyet a parallel overlay, not a beginner-friendly way to work on the real configuration, and it cannot edit existing definitions.

### Rewrite arbitrary Nix text with regexes

Rejected. It would corrupt comments, strings, imports, and nested expressions. Use a CST and refuse ambiguous edits.

### Leave repositories unnormalized

Rejected as the default. The parser could tolerate chaos, but every client would inherit ambiguity. Read-only scanning still tolerates it.

### Automatic migration during activation

Rejected. A boot-time rewrite of the configuration is not observable or reversible enough. Migration is an explicit, confirmed transaction.

### Give the helper arbitrary paths and content

Rejected. The helper accepts a plan scoped to a fixed configuration root, with relative operations and hashes. That is the narrowest capability that supports root-owned repositories.
