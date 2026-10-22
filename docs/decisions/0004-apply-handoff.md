# ADR 0004: Apply handoff and fixed managed state

Status: superseded for the target architecture by ADR 0005; kept as the record of the managed-module MVP.

> This ADR is historical. Where it conflicts with ADR 0005 or ADR 0006, the newer ADR wins.

## Context

The engine and CLI run as the invoking user. Applying a plan needs root to write system configuration and run `nh`. The helper must not become an arbitrary root file-write or command API, and the user's `/etc/nixos` must remain hand-written and authoritative.

The first target is a flake at `/etc/nixos` with a single host attribute.

## Decision

- The managed module lives at `/var/lib/nyet/nyet.nix`.
- nyet owns a wrapper flake at `/var/lib/nyet/flake.nix` that imports `path:/etc/nixos` and applies the managed module with `extendModules` to `nixosConfigurations.<host>`.
- The helper builds as the requesting user, then runs `nh os switch /var/lib/nyet#<host>` as root with a fixed environment.
- The CLI stages a plan under `/run/user/<uid>/nyet/plans` (mode `0600`, ten-minute lifetime) or `/run/nyet/plans/0` as root, then invokes the helper via `pkexec` with only the plan id.
- The helper validates the plan's version, id, owner (`PKEXEC_UID`), mode, size, expiry, host, and a restricted literal module grammar before writing.
- The helper serializes apply and rollback with an exclusive lock on `/var/lib/nyet/lock` and records transactions as JSON under `/var/lib/nyet/transactions`.
- Apply saves the previous module at `/var/lib/nyet/nyet.nix.previous`. Rollback is `nh os rollback` plus restoring that module.

## Module grammar

The helper accepts a fixed header and assignments whose values are literal strings, numbers, booleans, null, lists, attribute sets, or `pkgs.<attrpath>` references. It rejects functions, imports, `with`, `let`, string interpolation, operators, quoted option paths, and excessive nesting.

This is not a general Nix expression boundary. A user authorized to apply changes can still choose option values that execute code during a build; polkit authentication is the trust boundary. The grammar prevents a compromised unprivileged client from turning a plan into arbitrary evaluation or shell execution.

## Consequences

- No user import is required, and `/etc/nixos` is not edited by nyet.
- Apply is fixed to `/etc/nixos`; other flake paths and multi-host selection wait for first-run setup.
- Quoted option paths are rejected for apply.
- Rollback is one step, not arbitrary generation selection.
- The wrapper is rewritten on each apply so the host matches the validated plan.
- The plan spool is user-owned and readable by root, not root-owned. Ownership, mode, size, and expiry checks make the handoff safe.
