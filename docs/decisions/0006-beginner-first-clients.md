# ADR 0006: Beginner-first software-store clients

- Status: accepted
- Date: 2026-10-22

## Context

The primary audience is people arriving from imperative operating systems. They expect to install and remove applications from a software store, open a settings application, and see familiar choices. They do not necessarily know Nix, flakes, modules, or option paths.

nyet must provide that experience without replacing NixOS semantics. The configuration remains declarative, reviewable, and authoritative.

## Decision

### Software stores are the primary client

- The reference experience is a software store that can install and remove packages, search nixpkgs, and configure applications during installation.
- Multiple stores may exist for different desktop environments and toolkits. They all use the same engine protocol.
- The CLI remains the reference client for development, scripting, and debugging. It is not the primary end-user interface.
- A Qt/QML client remains the likely first graphical client because ztOS is an integration target, but it is not the product boundary.

### Package operations are configuration intents

- Install and uninstall are list-membership intents on `environment.systemPackages` or another configured package list.
- The engine shows the resulting Nix diff before apply.
- The engine never runs an imperative package installation or relies on `nix profile`.
- Package search and metadata are read operations. They do not mutate the system.

### Options have progressive disclosure tiers

- Every editable option may carry presentation metadata: `basic`, `intermediate`, or `advanced`.
- The tier is client-facing metadata, not configuration semantics. It does not change evaluation, validation, or the generated Nix.
- Basic views should cover common, safe, user-facing settings. Intermediate views add services and application configuration. Advanced views expose the remaining option surface and explicit expressions.
- Tiers are curated and versioned in the engine. Clients may present fewer options, but must not silently change an option's meaning.
- A client must always provide a way to see the exact Nix change and the option path.

### The product is training wheels

- The first-run flow should make the common path obvious: install apps, connect to Wi-Fi, enable a printer, set a hostname, choose a desktop, and configure applications.
- Every UI action maps back to a normal Nix definition in the user's repository.
- Users can inspect and edit the resulting Nix directly at any time.
- nyet must not trap users in a proprietary overlay. Removing nyet should leave a valid, understandable NixOS configuration.

### Assistant assistance is optional and untrusted

- An LLM or assistant may translate natural-language requests into proposed intents or edits.
- The proposal must be shown as a plan with the exact Nix diff.
- The engine validates types, paths, repository targets, and evaluation before anything is written.
- An assistant never receives direct write access, never bypasses confirmation, and never becomes the source of truth.
- Secrets and host-specific data must not be sent to an assistant by default.

## Consequences

Positive:

- The product meets beginners where they are: a software store and a settings application.
- Package installation remains declarative and tracked in configuration.
- Tiering keeps the large NixOS option surface approachable without hiding it permanently.
- One engine can serve multiple UI flavours.
- Users can graduate to hand-written Nix without losing their changes.

Negative:

- Curating tiers and first-run flows is ongoing product work, not a one-time feature.
- Software stores need package search, icons, descriptions, and progress UI beyond the core option model.
- Assistant integration adds privacy, prompt-injection, and validation risks.
- The engine must remain usable without an assistant; the assistant cannot be a required dependency.

## Alternatives considered

### CLI-first product

Rejected as the end-user experience. The CLI remains the reference client, but beginners should not need it.

### A single desktop-specific store

Rejected. It would repeat the coupling that nyet exists to avoid and make other desktops second-class.

### Imperative package installation with a configuration mirror

Rejected. It creates two sources of truth and can leave the configuration and the active system out of sync.

### Hide advanced options permanently

Rejected. Progressive disclosure should reduce noise, not prevent users from reaching the full option surface.
