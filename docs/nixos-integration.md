# NixOS integration

Status: draft. This document records intended behavior, not implemented behavior.

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

The index can be cached by:

- Flake lock revision.
- Nixpkgs revision.
- Host name.
- Engine schema version.

Invalidate the cache when the lock file or relevant configuration changes.

## Current values and provenance

Current values must be evaluated against the actual configuration.

Provenance should answer:

- Which file set this value?
- Is it a default, a user definition, or a module definition?
- Is it defined multiple times?
- Is it read-only?
- Is there a conflict with the managed module?

Do not promise full provenance until it is measured on real configurations. It may be expensive.

## Managed module

nyet writes a generated module, for example `/etc/nixos/nyet.nix`:

```nix
{ ... }:
{
  services.openssh.enable = true;
  networking.hostName = "phone";
}
```

The user imports it once:

```nix
imports = [
  ./hardware-configuration.nix
  ./nyet.nix
];
```

Rules:

- The module is generated deterministically.
- The module only contains options nyet owns.
- Removing an intent removes the assignment or resets it to the default, according to an explicit policy.
- If the user defines the same option elsewhere, surface a conflict instead of silently overriding.
- Keep a backup or generation reference before replacing the module.
- Never rewrite unrelated user files.

## Build and apply

The helper is responsible for privileged operations.

Expected operations:

1. Validate the plan and the requesting user.
2. Write the managed module atomically.
3. Run a build or switch with fixed arguments.
4. Record the resulting generation.
5. Return structured success or failure.

Do not pass the module content through a command-line argument. Use a root-owned spool file or stdin with a size limit.

The exact rebuild command depends on flake versus classic configuration and must be constructed by the helper, not by the frontend.

## Rollback

Rollback should target a recorded NixOS generation.

- Record the generation before apply.
- Record the generation after a successful apply.
- On failure, leave the previous generation active.
- On explicit rollback, invoke the fixed rollback operation and record it as a new transaction.

Do not reconstruct a previous configuration by replaying intents. Use the actual generation.

## First-run setup

The first-run flow must be explicit and reversible:

1. Detect the configuration path and host.
2. Verify nyet can evaluate it.
3. Offer to create and import the managed module.
4. Show exactly what will be written.
5. Require confirmation.
6. Record the setup as a transaction.

Do not silently edit `configuration.nix` or `flake.nix`.

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
- Whether nyet should manage the flake import automatically.
- How to handle packages and arbitrary expressions safely.
- How to test apply and rollback without risking the development host.
- How to represent secrets and sensitive values without logging them.
