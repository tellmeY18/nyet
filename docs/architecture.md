# Architecture

Status: draft.

Implementation note: the current code implements the managed-module MVP described in ADR 0004 plus a read-only repository scan and classic migration plan. This document describes the target repository-aware architecture from ADR 0005. Unbuilt pieces are marked planned.

## Problem

NixOS configuration is powerful but not approachable. Options are numerous, values are typed but often expressed as Nix, and changes live across flakes, host files, and imported modules. Existing graphical tools either embed the whole problem in one UI, write arbitrary files as root, or require the user to understand Nix.

The primary audience is people arriving from imperative operating systems. They expect a software store, a settings application, and clear choices. nyet must provide that experience without replacing the NixOS module system or creating a second source of truth.

nyet separates the reusable configuration engine from presentation:

- The repository remains the declarative source of truth.
- Evaluation remains the semantic source of truth.
- Clients speak intents; the engine owns parsing, evaluation, edits, plans, and transactions.
- A narrow privileged helper performs root-owned writes and activation.

## Product model

- Software stores are the primary end-user clients. The CLI is the reference client and debugging surface.
- Installing or removing software is a package-list intent, never an imperative package-manager operation.
- Common settings are presented through progressive disclosure tiers: `basic`, `intermediate`, and `advanced`.
- The first run scans the existing repository, explains what it found, and offers an opinionated migration to a supported layout.
- Every action produces normal, reviewable Nix in the user's repository. nyet is training wheels, not a proprietary overlay.

## Components

### Engine

The engine is an unprivileged process and library. It is the only component that understands the nyet configuration model.

Responsibilities:

- Discover the configuration root, flake or classic entrypoint, host, and imports.
- Parse every `.nix` file into a lossless syntax tree and index definitions and edit ranges.
- Evaluate NixOS options, current values, defaults, provenance, conflicts, and warnings.
- Build a repository model that maps evaluated options back to source definitions.
- Store pending intents and produce immutable plans.
- Render CST-aware edits for settings, package lists, and migrations.
- Validate plans by evaluation before apply.
- Stage plans and edit operations for the helper when the repository is root-owned.
- Record and read transactions, snapshots, and generations.
- Stream progress and build output to clients.

The engine does not write root-owned paths. It prepares a plan and asks the helper to apply root-owned edits and activation. It may write a user-owned repository directly.

The engine starts as a library inside `nyet-core`, consumed directly by the CLI. The long-running service and socket transport are part of the target architecture because software-store clients require them; they are not implemented yet.

### Repository index

The repository index is the bridge between text and evaluation.

It contains:

- Root path and configuration kind (flake or classic).
- Host names and the host selected for the session.
- Files, imports, and module entrypoints.
- Definitions with option paths, file paths, byte ranges, surrounding attributes, and conditions.
- The evaluated option index and selected-path provenance.
- Warnings for ambiguous, dynamic, or conflicting definitions.

Parsing is for structure and edit locations. It never supplies effective values. A definition that cannot be mapped safely is marked uneditable rather than guessed.

Use an existing lossless parser (`rnix`/`rowan` or equivalent). Do not implement a Nix grammar.

### Migration

Migration is an explicit first-run transaction that normalizes a repository into:

```text
flake.nix
hosts/<host>/configuration.nix
hosts/<host>/hardware-configuration.nix
modules/...
```

It changes structure, not semantics:

- Preserve existing option definitions, package lists, comments, and imported modules.
- Generate a flake entrypoint for a classic configuration when necessary.
- Move or create host and hardware entrypoints and update imports.
- Never rewrite arbitrary expressions merely to fit a template.
- Snapshot affected files, evaluate old and candidate configurations, show a full diff, and require confirmation.
- Apply atomically and record an exact rollback snapshot.
- Never migrate during system activation. `services.nyet.enable` may start the engine; `nyet init` or a client onboarding flow performs the migration.

Read-only scanning works on unmigrated repositories. Migration is required for managed edits, not for inspection.

### Helper

The helper is a small root process invoked through polkit. It must remain auditable.

Allowed operations:

- Validate a plan id, owner, mode, size, expiry, host, plan kind, relative paths, before-hashes, and module grammar.
- Apply a scoped repository edit or migration plan under one fixed configuration root.
- Write the legacy managed module and wrapper during the MVP transition.
- Build as the requesting user, then run a fixed `nh os switch` as root.
- Record a transaction and generation reference.
- Roll back one generation and restore snapshot files.

It must not:

- Accept arbitrary paths from a frontend.
- Accept whole configuration files from a frontend.
- Accept a path outside the configured root.
- Follow symlinks or traversal out of the configuration root.
- Evaluate untrusted Nix supplied by a client.
- Run a shell.
- Expose a general-purpose root API.

The helper receives an opaque plan id. The engine stages the plan under `/run/user/<uid>/nyet/plans` (mode `0600`, ten-minute lifetime; `/run/nyet/plans/0` as root) and the helper reads it as root. The helper validates that the file is regular, owned by the authenticated user, and mode `0600` or stricter.

### Frontends

Frontends are clients of the engine protocol. They own presentation and interaction only.

Planned clients:

- Software stores for multiple desktop environments and toolkits.
- CLI: reference client, scripting surface, debugging tool.
- Qt/QML: first graphical client and ztOS integration target.
- GTK4: desktop client.
- Web: remote or local browser UI.
- TUI: terminal UI for servers and rescue environments.

No frontend should invoke `nh`, write the configuration root, or perform privileged operations directly.

### Crate map

Dependency rule: `nyet-cli -> nyet-core`; `nyet-helper -> nothing`; `nyet-protocol` depends only on wire types.

| Crate | One-sentence contract | Owns | Must never |
|---|---|---|---|
| `nyet-core` (lib) | The only component that understands NixOS configuration: read it, model it, propose changes, and edit it. | Repository discovery and indexing; CST parsing; option evaluation and cache; typed values; intents; validation; configuration and migration plans; diffs; transaction records. | Depend on argv, sockets, UI, or root paths; infer effective configuration from text; write root-owned paths. |
| `nyet-cli` (bin `nyet`) | Reference client and debugging surface; a thin interpreter of core calls. | Subcommands, human and JSON output, exit codes; a client of the engine socket later. | Contain configuration logic, evaluate Nix itself, or edit files directly. |
| `nyet-helper` (bin `nyet-helper`) | Tiny privileged executor with a fixed, auditable operation set. | Validate plan ids, ownership, mode, size, expiry, relative paths, hashes, and module grammar; apply scoped edits; run a fixed rebuild; record transactions; roll back. | Accept arbitrary paths, whole configurations, or commands; link `nyet-core`; evaluate Nix; run a shell. |
| `nyet-protocol` (planned) | Wire types and JSON-RPC schema only. | Method and event types, versioning, error codes, capability discovery. | Contain configuration logic, parsing, evaluation, or privilege. |

## Data model

### Repository

The discovered configuration root and its structure:

- Root path and kind.
- Flake entrypoint, host attributes, and selected host.
- Files and imports.
- Definitions and their edit ranges.
- Migration status and warnings.

### OptionPath

A stable dotted path such as `services.openssh.enable`. The path is the identity of an option.

### OptionSpec

Metadata from NixOS evaluation:

- Path.
- Type.
- Description.
- Default.
- Example.
- Read-only flag.
- Declarations and definitions.
- Visibility and internal flags when available.
- UI tier metadata, if curated.

### Value

A typed value:

- Boolean.
- Integer.
- Float.
- String.
- Enum.
- Path.
- Package reference.
- List.
- Attribute set.
- Submodule.
- Opaque Nix expression, used only when necessary.

Values carry their type so a frontend can render the right control and the engine can validate writes.

### Definition

A source location that contributes to an option or package list:

- Option path.
- File and byte range.
- Attribute path and expression.
- Whether it is a direct assignment, list entry, or conditional contribution.
- Import chain and module scope when known.
- Whether the definition is safely editable.

Definitions come from the CST index and are correlated with evaluated provenance. Dynamic or ambiguous definitions are not editable.

### Intent

A pending desired change. Intents are not applied until the user creates a plan.

An intent records:

- Option path or package list.
- Operation: set a value, reset to the default, add a list member, or remove a list member.
- Desired value or member.
- Optional explicit edit target (definitions id/file) chosen by the client.
- Source client.
- Timestamp.

There is no enable, install, start, or shell-out intent. Package changes are list-membership intents on a list option such as `environment.systemPackages`.

### Plan

An immutable description of what would change.

A plan contains:

- Plan identifier and kind (configuration, migration, or activation).
- Base configuration revision and file hashes.
- Intents or migration operations.
- Exact file edits, including paths, ranges, before-hashes, and replacement text.
- Affected option paths.
- Build derivation, when available.
- Warnings and validation errors.
- Confirmation token.
- Creation time and expiry.

A plan never mutates the system or the repository.

### Transaction

A record of an attempted apply:

- Transaction identifier.
- Plan identifier and kind.
- Actor.
- Start and finish times.
- Build-log reference.
- Files changed and their backups.
- Result.
- Resulting generation.
- Rollback generation or migration snapshot.

### Generation

A NixOS system generation reference. Rollback targets a recorded generation and restores the matching configuration snapshot.

## Declarative write model

Every change becomes declarative Nix in the repository. There is no imperative escape hatch in the core or the protocol.

- A set intent edits the selected definition or inserts `path = value;` into the chosen target.
- A reset intent removes the assignment so the NixOS default applies.
- A list add intent inserts a member into the selected package list.
- A list remove intent removes the selected member from the selected list.
- Edits are CST-aware and preserve comments, indentation, and unrelated text.
- Multiple edits are applied deterministically and reviewed as one diff.
- If the same option is defined in multiple places, the plan either targets an explicitly chosen definition or refuses the edit as ambiguous.
- If the user's own modules define the same option multiple times, the plan shows every definition and does not silently override one.
- If a package list does not exist, the engine may add it to the chosen host file as part of the reviewed plan.
- nyet never creates a hidden settings module. If a new definition is needed, it goes into the selected user file.

The repository stays declarative because every change is ordinary Nix, visible in version control, and removable without nyet.

## Read flow

1. Client calls `engine.capabilities`.
2. Client calls `repo.scan` to discover the root, host, layout, files, and migration status.
3. Client calls `options.index` or a paginated/filtered variant.
4. Client calls `options.get` or `config.evaluate` for current values.
5. Client calls `config.definitions` when it needs editable targets.
6. The engine evaluates against the discovered configuration and correlates results with the CST index.
7. Results are cached by repository fingerprint, host, and schema version.

Evaluation must not block the protocol loop. Long reads run in a worker and stream progress or completion events.

## Write flow

1. Client sets one or more intents, optionally selecting an edit target.
2. Client requests `plan.create`.
3. Engine validates intents against the option model and repository definitions.
4. Engine selects or verifies edit targets and renders CST edits.
5. Engine evaluates the candidate configuration.
6. Engine returns a plan with the exact diff, warnings, and a confirmation token.
7. Client calls `plan.apply` with the plan identifier and token.
8. For a user-owned repository, the engine writes atomically with backups, then invokes the helper to activate.
9. For a root-owned repository, the engine stages the plan and the helper applies the scoped edits, then activates.
10. On success, the transaction and generation are recorded.
11. On failure, changed files are restored from backups; the active generation is unchanged and the failure is recorded.

Migration uses the same plan and transaction machinery, with plan kind `migration` and a full snapshot.

## Migration flow

1. Client or `nyet init` requests `repo.scan`.
2. Engine reports the detected kind, host, files, and whether migration is required.
3. Client requests `repo.migration.plan`.
4. Engine builds a structural migration: entrypoints, host directory, hardware import, and import wiring.
5. Engine snapshots every affected file and evaluates the candidate.
6. Engine returns the full file diff, warnings, and confirmation token.
7. Client calls `repo.migration.apply`.
8. Engine or helper writes atomically under the same locking and path rules as configuration edits.
9. Engine records the migration snapshot.
10. On any failure, the exact original files are restored.

Migration never activates a new system generation by itself. Activation is a separate apply.

## Security boundary

The frontend is untrusted from the helper's point of view.

- The helper is invoked by the engine, not by a remote frontend.
- The helper receives an opaque plan id, not paths or content from the client.
- The helper accepts only fixed plan kinds and a fixed configuration root.
- Relative edit paths are canonicalized under that root. Traversal, absolute paths, symlink escapes, and non-regular files are rejected.
- Before-hashes prevent applying a plan against modified files.
- The helper uses a fixed activation command and a fixed environment.
- The helper serializes apply, migration, and rollback with an exclusive lock.
- Build output is inherited by the invoking terminal; it is not captured or uploaded by default.
- Assistant proposals are untrusted and must become ordinary plans.

Polkit identifies the requesting user through `PKEXEC_UID`. The helper verifies that the plan belongs to that user and has not expired. Any user authorized to apply NixOS changes can still choose option values that execute code during a build; polkit authentication, not the grammar, is the trust boundary.

User-owned repositories are outside the root boundary: the engine edits them as the invoking user and only the helper performs activation. The helper must never write a path outside its configured root.

## Evaluation strategy

NixOS evaluation is the source of truth. The engine must not infer effective configuration by parsing text.

The engine shells out to `nix eval --json --impure` with embedded expressions that use `builtins.getFlake`. The bulk index carries path, type, description, `defaultText`, flags, and declarations. Arbitrary defaults are not forced in bulk because some NixOS defaults throw; a typed default is fetched per option only when it is safe. Values and defaults are sanitized before serialization: functions become null, derivations become their `outPath`, and paths become strings.

The index is cached under the nyet cache directory. The cache key includes the canonical root path, the lock file and configuration contents, the host, and an index schema version. A write-through cache is best effort: cache failures never fail a read.

Provenance is fetched for selected paths, not the bulk index, because forcing `isDefined` on every option can force throwing defaults. Definitions under the flake's nixpkgs input are filtered out; remaining definition files are treated as user or external-module definitions for conflict warnings.

The CST index supplies file paths and byte ranges. Evaluation supplies effective values and provenance. The two are correlated but not substituted for one another.

See [ADR 0003](decisions/0003-option-evaluation.md) and [ADR 0005](decisions/0005-repo-aware-editing-and-migration.md).

Expected inputs:

- The user's configuration root, flake or classic.
- The selected host.
- The nixpkgs revision and lock file state.

Expected outputs:

- Repository structure and migration status.
- Option metadata.
- Current values for requested paths.
- Defaults.
- Definitions, declarations, and editable ranges.
- Evaluation warnings and errors.

Full configuration JSON is not a good protocol payload: it can be huge and may contain non-serializable values. Prefer an option index plus targeted value queries, with a bounded bulk-read operation for the first screen.

Open questions:

- Which evaluation mechanism is fastest and most reliable across flakes, channels, and older NixOS releases?
- How much provenance can be extracted without making every read prohibitively slow?
- How should values that are functions, derivations, or packages be represented to frontends safely?
- How should edits behave when a definition is generated by `mkIf` or `mkMerge`?

## Failure handling

- Scan failure: return structured errors and do not offer edits.
- Evaluation failure: return structured errors; keep the previous plan.
- Migration evaluation failure: do not apply; keep the snapshot and report the diff.
- Edit conflict or stale hash: reject the plan and ask the client to rescan.
- Dry-build failure: keep the plan for inspection; do not apply.
- Apply failure: keep the previous generation; restore edited files; record the failed transaction and logs.
- Interrupted apply or migration: on next startup, reconcile the transaction record with the active generation and file hashes.
- Corrupt state: fail closed and report, rather than guessing.

## Testing strategy

Core behavior is tested through the library against fixture repositories, never the developer's host. Repository scans, CST edits, migration output, and diffs are protected by golden tests. Privileged behavior is kept behind the helper boundary and verified in a Linux VM.

Tests are part of done for every milestone. See [testing.md](testing.md).

## Open design questions

- Exact CST library and edit-correlation strategy.
- How to present and resolve multiple definitions of the same option.
- How much of Home Manager and nix-darwin to support in the first version.
- Whether the engine service is per-user or system-wide on multi-user machines.
- How to detect and safely migrate classic configurations without manual intervention.
- How to curate option tiers and keep them current across nixpkgs releases.
- How to package software-store metadata without making the engine a package manager.
- How to validate assistant proposals against prompt injection and stale repository state.
