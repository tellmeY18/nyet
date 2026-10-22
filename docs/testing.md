# Testing

Status: 66 tests on Darwin (67 on Linux) across core unit, helper unit, fixture evaluation, golden render, and CLI integration, plus a green gated x86_64-linux NixOS VM test run by CI. Protocol tests are pending.

Tests are part of the definition of done for every milestone. They are written with the behavior they protect, not in a later cleanup pass.

## Principles

- Test behavior through public interfaces, not private implementation details.
- The default suite runs without root, without network, and without mutating a real NixOS configuration.
- Nix-backed tests use fixture configurations under `tests/fixtures/`, never the developer's host.
- Slow or privileged tests are gated and run in CI, not silently skipped.
- Every bug fix gets a regression test.
- Golden files are reviewed like code; changes to rendered Nix or diffs must be intentional.

## Test layers

| Layer | What it protects | Where | Default run |
|---|---|---|---|
| Unit | Value model, path parsing, type coercion, validation, list membership, rendering, diffing, cache keys, intent store, errors | Next to the code in `nyet-core` | Yes |
| Fixture evaluation | Real option index, current values, provenance, cache writes, candidate-module check | `tests/fixtures/` | Yes, requires `nix` |
| Golden | Exact managed-module text and plan diffs | `tests/golden/` | Yes |
| CLI integration | Subcommands, intent persistence, JSON output, exit codes | `crates/nyet-cli/tests/` | Yes |
| Protocol | Framing, versioning, error codes, round-trips | `nyet-protocol` (later) | Yes |
| Helper | Module grammar, path and host allowlists, plan ids, atomic writes, wrapper rendering | `nyet-helper` unit tests | Yes |
| VM | Plan staging, ownership and expiry, apply, rebuild, rollback, transaction records | `tests/vm/` | No; Linux CI only |

## Fixtures

A fixture is a self-contained flake with its own `flake.lock`, for example:

```text
tests/fixtures/basic/
  flake.nix
  flake.lock
  configuration.nix
```

Rules:

- A fixture must evaluate without the developer's host configuration.
- Pin nixpkgs in the fixture lock file; do not follow the repository flake implicitly.
- Keep fixtures small. One fixture per behavior cluster; add a new fixture rather than growing one into a kitchen sink.
- Never point a test at `/etc/nixos` or a home directory.
- Tests must not write to the fixture; render to a temporary directory and compare.

## Nix-backed tests

- Option evaluation must be exercised against a fixture, not a hand-written JSON blob.
- A test that needs a Linux-only build is marked `#[ignore]` and run by CI, or lives in the VM suite.
- Evaluation that works on `aarch64-darwin` should run in the default suite; building a NixOS system must not.
- Do not mock Nix evaluation in core tests. Mock only process boundaries that cannot be exercised locally.

## Required coverage

- Every public core behavior has a happy-path and an error-path test.
- Every intent operation (`set`, `reset`, `list_add`, `list_remove`) has validation and rendering tests.
- Every structured error variant is reachable by a test.
- Every CLI subcommand has at least one integration test, including a failure case and a stable JSON shape.
- Rendered modules and plan diffs have golden tests.
- Cache keys change when configuration changes, and stale entries are ignored.
- Intent storage survives a reload and persists list membership.
- The helper's validation rejects bad plan ids, wrong owners, broad plan modes, oversized or expired plans, unsafe hosts, and executable or deeply nested Nix.
- Helper tests cover atomic module writes and the fixed wrapper text without requiring root.
- The VM suite is the only place that runs a real rebuild, apply, and rollback.

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
