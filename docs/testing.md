# Testing

Status: 73 tests on Darwin (74 on Linux) across core unit, repository scan and migration-plan tests, helper unit, fixture evaluation, golden render, and CLI integration, plus a gated x86_64-linux NixOS VM test run by CI. Definition, migration-apply, and protocol tests are planned.

Tests are part of the definition of done for every milestone. They are written with the behavior they protect, not in a later cleanup pass.

## Principles

- Test behavior through public interfaces, not private implementation details.
- The default suite runs without root, without network, and without mutating a real NixOS configuration.
- Nix-backed tests use fixture configurations under `tests/fixtures/`, never the developer's host.
- Repository and migration tests use disposable fixture repositories, not `/etc/nixos`.
- Slow or privileged tests are gated and run in CI, not silently skipped.
- Every bug fix gets a regression test.
- Golden files are reviewed like code; changes to rendered Nix, edits, migrations, or diffs must be intentional.

## Test layers

| Layer | What it protects | Where | Default run |
|---|---|---|---|
| Unit | Value model, path parsing, type coercion, validation, list membership, rendering, diffing, cache keys, intent store, errors | Next to the code in `nyet-core` | Yes |
| Repository | Root discovery, file walk, import graph, syntax errors, standard-layout detection, migration-plan generation | Core unit tests with disposable trees | Yes |
| CST edits | In-place assignment edits, list insertion/removal, comment and formatting preservation, stale hash rejection | `nyet-core` plus golden files | Yes |
| Fixture evaluation | Real option index, current values, provenance, cache writes, candidate-module check | `tests/fixtures/` | Yes, requires `nix` |
| Golden | Exact rendered modules, repository edits, migration output, and plan diffs | `tests/golden/` | Yes |
| Migration | Classic and flake normalization, idempotency, ambiguity refusal, evaluation, exact rollback | `tests/fixtures/repos/` and golden files | Yes |
| CLI integration | Subcommands, intent persistence, JSON output, exit codes | `crates/nyet-cli/tests/` | Yes |
| Protocol | Framing, versioning, error codes, round-trips | `nyet-protocol` (later) | Yes |
| Helper | Module grammar, relative-path validation, hashes, plan ids, atomic writes, wrapper rendering, rebuild handoff | `nyet-helper` unit tests | Yes |
| VM | Plan staging, ownership and expiry, non-root build, apply, rebuild, rollback, migration, transaction records | `tests/vm/` | No; Linux CI only |

## Fixtures

Two fixture families are required.

### Configuration fixtures

A configuration fixture is a self-contained flake with its own `flake.lock`, for example:

```text
tests/fixtures/basic/
  flake.nix
  flake.lock
  configuration.nix
```

### Repository fixtures

A repository fixture is a disposable tree used for scanning, editing, and migration:

```text
tests/fixtures/repos/classic/
  configuration.nix
  hardware-configuration.nix

tests/fixtures/repos/unormalized-flake/
  flake.nix
  hosts/phone/default.nix
  modules/desktop.nix

tests/fixtures/repos/ambiguous/
  flake.nix
  configuration.nix
  modules/override.nix
```

Rules:

- A fixture must evaluate without the developer's host configuration.
- Pin nixpkgs in the fixture lock file; do not follow the repository flake implicitly.
- Keep fixtures small. One fixture per behavior cluster; add a new fixture rather than growing one into a kitchen sink.
- Never point a test at `/etc/nixos` or a home directory.
- Tests must not write to the source fixture; copy it to a temporary directory and edit the copy.
- Migration tests compare against golden output and verify exact restoration on failure.

## Nix-backed tests

- Option evaluation must be exercised against a fixture, not a hand-written JSON blob.
- Repository parsing and edits do not require Nix evaluation, but their final candidate must evaluate where the behavior depends on semantics.
- A test that needs a Linux-only build is marked `#[ignore]` and run by CI, or lives in the VM suite.
- Evaluation that works on `aarch64-darwin` should run in the default suite; building a NixOS system must not.
- Do not mock Nix evaluation in core tests. Mock only process boundaries that cannot be exercised locally.

## Required coverage

- Every public core behavior has a happy-path and an error-path test.
- Every intent operation (`set`, `reset`, `list_add`, `list_remove`) has validation, target-selection, and rendering tests.
- Every structured error variant is reachable by a test.
- Every CLI subcommand has at least one integration test, including a failure case and a stable JSON shape.
- Rendered modules, CST edits, migration output, and plan diffs have golden tests.
- Cache keys change when the repository changes, and stale entries are ignored.
- Intent storage survives a reload and persists list membership.
- Repository scans classify every definition as editable or uneditable with a reason.
- Edits preserve comments, indentation, and unrelated attributes; stale before-hashes are rejected.
- Multiple definitions produce an ambiguity error unless a target is selected.
- Migration is idempotent and leaves semantically equivalent configuration.
- Migration failure restores every original file byte-for-byte.
- Package install and uninstall change only the selected package list.
- Package search never mutates configuration.
- The helper's validation rejects bad plan ids, wrong owners, broad plan modes, oversized or expired plans, unsafe hosts, absolute or traversal paths, symlink escapes, stale hashes, and executable or deeply nested Nix.
- Helper tests cover atomic file writes and the fixed wrapper text without requiring root.
- The VM suite is the only place that runs a real rebuild, apply, migration, and rollback.

## Commands

```bash
nix develop --command cargo fmt --all --check
nix develop --command cargo clippy --workspace --all-targets -- -D warnings
nix develop --command cargo test --workspace
nix develop --command cargo test --workspace -- --ignored
```

The last command is for gated tests that are safe to run on the current host. Linux VM tests run in CI.

## CI

GitHub Actions runs formatting, clippy, the default test suite, and `checks.x86_64-linux.vm` on every push and pull request. The VM job uses QEMU; it runs with KVM when the runner exposes `/dev/kvm`, otherwise with TCG. The workflow is `.github/workflows/ci.yml`.
