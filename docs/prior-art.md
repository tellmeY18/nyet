# Prior art

Status: research notes.

The idea of a NixOS configuration GUI is not new. The gap is a reusable, safe engine with multiple frontends.

## Summary

| Project | State | What it does | Why it is not the base |
|---|---|---|---|
| [snowfallorg/nixos-conf-editor](https://github.com/snowfallorg/nixos-conf-editor) | 686 stars, GPL-3.0, Rust/GTK4, last push Feb 2026 | Option editor with rebuild flow | No reusable engine API; GTK-only; thin root helper accepts config content and paths |
| [nix-gui/nix-gui](https://github.com/nix-gui/nix-gui) | 763 stars, last push Sep 2022 | Broad option editor | Stale; copies configuration; incomplete types |
| [pmiddend/nixos-manager](https://github.com/pmiddend/nixos-manager) | 194 stars, last push 2020 | Packages and services UI | Old; fixed file layout; no reusable core |
| [flooryyyy/nixos-gui](https://github.com/flooryyyy/nixos-gui) | 1 star, created 2026 | Options, package search, dry-build/test/switch | Very new experiment; web UI; no established API |
| [milahu/nixos-config-webui](https://github.com/milahu/nixos-config-webui) | 11 stars, last push 2022 | Web configuration prototype | Abandoned proof of concept |
| [snowfallorg/nix-software-center](https://github.com/snowfallorg/nix-software-center) | Active, Rust/GTK4 | Package store | Not a configuration manager |
| [mynixos.com](https://mynixos.com/) | Web service | Build/share configurations | Not a local system manager |

The NixOS Wiki maintains a list: [NixOS configuration editors](https://wiki.nixos.org/wiki/NixOS_configuration_editors).

## nixos-conf-editor notes

Observed in the repository:

- Rust, Relm4, GTK4/libadwaita.
- GPL-3.0.
- Workspace with the GUI and `nce-helper`.
- `src/parse/config.rs` uses `nix-editor` to parse and rewrite Nix text.
- `src/parse/options.rs` loads option metadata from JSON.
- `src/ui/rebuild.rs` invokes `pkexec nce-helper write-rebuild --content <config> --path <path> -- switch ...`.
- `nce-helper/src/main.rs` requires root, writes a supplied path, and runs `nixos-rebuild`.

Implications:

- It is a useful reference for option UI and rebuild flow.
- It is not a headless engine.
- Its helper trusts the GUI for content and path.
- Its text editing is a useful reference for CST-based definition edits, but it does not correlate edits with evaluated provenance or offer an ambiguity policy.
- Whole configuration content is passed through a command line.
- There is no plan, confirmation token, transaction, or rollback model in the engine.

A fork could be refactored into an engine plus GTK frontend, but that is a substantial rewrite of the privileged path and the data model. A clean engine with a CLI first is less likely to inherit the coupling.

## What is still missing

None of the surveyed projects clearly combines:

- Evaluation-backed option discovery and current values.
- A stable API usable by multiple frontends.
- Repository-aware edits to existing definitions, with CST preservation, ambiguity refusal, snapshots, and rollback.
- Plan, dry-build, apply, transaction, and rollback.
- A narrow privileged helper with fixed paths and operations.
- Package management as one provider rather than the whole product.

That combination is the target.
