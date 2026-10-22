# ADR 0003: Option evaluation through `nix eval`

- Status: accepted; amended by ADR 0005
- Date: 2026-10-22
- Amendment: evaluation remains the semantic source of truth. ADR 0005 adds CST parsing for file structure, provenance, and edit ranges; parsing must not infer effective values.

> The original decision text below is retained for history. Where it conflicts with ADR 0005 or ADR 0006, the newer ADR wins.

## Context

The engine must read the evaluated NixOS option surface and current values. It must not parse Nix text. The POC runs on `aarch64-darwin` against a fixture flake whose `nixosConfigurations.basic` targets `x86_64-linux`, so evaluation must work without building a Linux system.

Several approaches were tried:

- Evaluating `config.system.build.manual.optionsJSON` builds a derivation, which is not portable from the development host.
- `builtins.toJSON` on a raw option set fails on functions, derivations, and other values that cannot be serialized.
- Wrapping `toJSON` in `builtins.tryEval` does not catch type errors such as "cannot convert a function to JSON" on the tested Nix version.
- Forcing every option default in bulk fails because some defaults throw (for example, removed packages or options that reference a missing package).
- `builtins.fromJSON` refuses strings that refer to store paths in this evaluation mode.

## Decision

Evaluate through `nix eval --json --impure` with small embedded Nix expressions and `builtins.getFlake`.

- The bulk option index returns path, type, description, `defaultText`, `readOnly`, `internal`, `visible`, and declarations.
- The index does not force arbitrary defaults. `defaultText` is returned as text when present.
- A typed default is fetched in a separate evaluation for a single option, only when it is a simple scalar type and no `defaultText` exists. If that evaluation fails, the default is omitted.
- Current values are fetched per option path with `lib.getAttrFromPath` and sanitized before serialization.
- Sanitization is done in Nix: functions become null, derivations become `{ outPath = ...; }`, paths become strings, and lists and attrsets are mapped recursively.
- `visible` is normalized because NixOS uses booleans and the strings `"shallow"` and `"transparent"`.
- The engine captures the Nix process exit status and stderr. A failed evaluation is a structured engine error; no partial index is returned.

M1 adds a write-through index cache. The key is the canonical flake path, the host, a fingerprint of `flake.lock` and the flake's `.nix` files, and an index schema version. Cache I/O is best effort; a miss or failure falls back to evaluation.

Provenance is fetched only for selected paths. The bulk index does not read `isDefined` because forcing it can force a throwing default (for example `hardware.nvidia.open`). The selected-path query filters definition locations under the flake's nixpkgs input and returns the remaining files as user or external-module definitions.

## Consequences

Positive:

- Works on the primary development host without building Linux derivations.
- No Nix text parsing and no parallel configuration database.
- A failed option value or default cannot corrupt the whole index.
- The expression is small enough to audit and test against a fixture.

Negative:

- `nix eval --impure` requires flakes and impure evaluation, and is slower than a cached index.
- Bulk defaults are incomplete; frontends may see `defaultText` instead of a typed default.
- Per-option default evaluation adds a process per `get`.
- Sanitization is lossy for functions, derivations, and complex attrsets.
- Nix's `tryEval` is not a general error boundary, so the design avoids forcing values it cannot safely render.

## Alternatives considered

### Build `optionsJSON`

Rejected for the POC. It is the closest thing to a supported option index, but it is a derivation built with Linux packages, which the development host cannot build. Revisit if a Linux-only engine or CI path becomes primary.

### Parse Nix source

Rejected by the architecture invariants. Evaluation is the source of truth.

### Evaluate and serialize the whole configuration

Rejected. It is large, contains non-serializable values, and couples clients to internal config shape.

### Cache the option index now

Deferred during the POC, then implemented in M1. The cache is keyed by flake contents, host, and schema version, not by a process-lifetime daemon.

## Follow-ups

- Measure index and per-value latency on a real host.
- Revisit `optionsJSON` once Linux CI can build it.
- Consider a long-running evaluation worker if per-value queries dominate.
- `OptionPath` does not yet parse quoted segments such as `boot.kernel.sysctl."kernel.hostname"`; add this when the read model is hardened.
- Evaluate full user-vs-module provenance; current conflict warnings filter nixpkgs definitions but do not classify every source.
