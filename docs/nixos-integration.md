# NixOS integration

Status: draft. Read, plan, and the MVP apply/rollback path are implemented. Apply and rollback are not yet verified in a disposable NixOS VM.

## Source of truth

The user's NixOS configuration is authoritative. nyet does not maintain a separate database of effective configuration.

nyet owns:

- Pending intents.
- Plans.
- A generated managed module.
- Transaction and generation metadata.

nyet does not own:

- The user's hand-written modules.
- Nixpkgs.
- The Nix store.
- The active system generation.

## Configuration targets

The first target is a flake-based NixOS configuration:

```text
/etc/nixos/flake.nix
/etc/nixos/configuration.nix
```

The engine needs:

- The flake path.
- The host attribute name, for example `.#nixosConfigurations.phone`.
- The current system generation.

Classic non-flake configurations can be supported after the flake path works.

## Option discovery

Options must come from evaluation, not from parsing source files.

The engine should produce or consume an option index containing:

- Option path.
- Type.
- Description.
- Default.
- Example.
- Read-only flag.
- Declarations.
- Definitions.

The index is cached by:

- Canonical flake path.
- Flake lock and `.nix` configuration contents.
- Host name.
- Engine schema version.

The cache is invalidated when the fingerprint changes. It is best effort and never required for correctness.

## Current values and provenance

Current values must be evaluated against the actual configuration.

Provenance should answer:

- Which file set this value?
- Is it a default, a user definition, or a module definition?
- Is it defined multiple times?
- Is it read-only?
- Is there a conflict with the managed module?

Do not promise full provenance until it is measured on real configurations. It may be expensive.

## Managed module and wrapper

nyet writes a generated module at `/var/lib/nyet/nyet.nix`:

```nix
{ config, lib, pkgs, ... }:
{
  services.openssh.enable = true;
  networking.hostName = "phone";
}
```

The user does not import it into `/etc/nixos`. The helper owns a wrapper flake at `/var/lib/nyet/flake.nix`:

```nix
{
  inputs.user.url = "path:/etc/nixos";
  outputs = { self, user }: {
    nixosConfigurations."phone" =
      user.nixosConfigurations."phone".extendModules {
        modules = [ ./nyet.nix ];
      };
  };
}
```

This keeps `/etc/nixos` authoritative and hand-written while nyet owns only its own state directory. The module path is relative so flake evaluation stays pure. The wrapper is rewritten before each apply so the host name always matches the plan.

Rules:

- The module is generated deterministically.
- The module only contains options nyet owns.
- Removing an intent removes the assignment or resets it to the default, according to an explicit policy.
- If the user defines the same option elsewhere, surface a conflict instead of silently overriding.
- Keep `/var/lib/nyet/nyet.nix.previous` before replacing the module.
- Never rewrite unrelated user files.
- The module must match a restricted literal grammar: a fixed header, unquoted option paths, and literal strings, numbers, booleans, null, lists, attribute sets, or `pkgs.<attrpath>` references. No functions, imports, `with`, `let`, string interpolation, or operators.

## Declarative translation

Every requested change is translated into a declarative assignment in the managed module. nyet never applies an imperative operation.

| Intent | Managed module result |
|---|---|
| set `services.openssh.enable = true` | `services.openssh.enable = true;` |
| reset `services.openssh.enable` | assignment removed; the NixOS default applies |
| list add `environment.systemPackages` | the member is added to the generated list and merges with user lists |
| list remove `environment.systemPackages` | the member is removed from the generated list only |

- Rendering is deterministic: intents are sorted by option path.
- The plan diff shows the exact managed-module change.
- A conflicting hand-written definition is reported as a warning, never silently overridden.
- Adding or removing a package is a list-membership intent, not an install command.
- The managed module binds `config`, `lib`, and `pkgs` so package values resolve.
- `nyet plan --check` evaluates the candidate module to `system.build.toplevel.drvPath`. It is an evaluation check, not a build.

The user's repository stays declarative because the only file nyet owns is the managed module.

## Build and apply

The CLI stages a plan under `/run/user/<uid>/nyet/plans` (mode `0600`, ten-minute lifetime; `/run/nyet/plans/0` as root) and passes only the plan id to the helper through `pkexec`. The helper validates:

- The id is a 32-character lowercase hex string.
- The plan file is regular, owned by `PKEXEC_UID`, mode `0600` or stricter, and no larger than 1 MiB.
- The plan version, id, uid, host, creation time, and expiry.
- The module grammar above.

It then:

1. Writes the wrapper flake and managed module atomically.
2. Saves the previous module at `/var/lib/nyet/nyet.nix.previous`.
3. Runs `/run/current-system/sw/bin/nixos-rebuild switch --flake /var/lib/nyet#<host>` with a fixed environment.
4. On failure, restores the previous module and wrapper.
5. Records a transaction under `/var/lib/nyet/transactions`.
6. Removes the plan on success.

Do not pass module content through a command-line argument. The helper never accepts a path, content, or command from the client.

Limits: apply requires a flake at `/etc/nixos`, a host name made of letters, digits, `_`, or `-`, and unquoted option paths. Quoted paths such as `boot.kernel.sysctl."kernel.hostname"` are rejected for apply.

## Rollback

Rollback is one step:

- Record the generation before and after each apply.
- On failure, leave the active generation unchanged and restore the previous module.
- On explicit rollback, run `nixos-rebuild switch --rollback`, restore `/var/lib/nyet/nyet.nix` from `.previous`, and record a new transaction.

The helper serializes apply and rollback with an exclusive lock on `/var/lib/nyet/lock`. Arbitrary recorded-generation rollback and build-log capture are not implemented. Do not reconstruct a previous configuration by replaying intents.

## First-run setup

Not implemented. Today apply is gated on a fixed `/etc/nixos` flake and on the nyet NixOS module being enabled (`programs.nyet.enable = true`), which installs both binaries and the polkit rule.

The first-run flow must be explicit and reversible:

1. Detect the configuration path and host.
2. Verify nyet can evaluate it.
3. Show the exact wrapper, managed module, and first plan.
4. Require confirmation.
5. Record the setup as a transaction.

Do not silently edit `configuration.nix` or `flake.nix`.

## Testing

Read behavior and rendering are tested against fixture flakes under `tests/fixtures/`, never a real host. Apply and rollback are verified in a disposable Linux VM and gated out of default test runs.

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
- How to verify apply and rollback on a disposable NixOS VM without risking the development host.
- How to present and safely support arbitrary expressions beyond `pkgs.<attrpath>`.
- How to make rollback target an arbitrary recorded generation instead of one step.
- How to represent secrets and sensitive values without logging them.
