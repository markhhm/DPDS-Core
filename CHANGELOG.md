<!-- SPDX-License-Identifier: Apache-2.0 -->

# Changelog

All notable changes to this fork are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). No version of the HNG
fork has been released yet.

## [Unreleased]

### Added

- Actual engine-output late-run and conditional-lateness measurements for trace
  and fixed two-point modes; a separately marked fidelity-tolerance proposal.
- Offline `trace_replay` HngBuffer executable and an independent Fraction oracle
  checking converted synthetic and external real-Garcia per-packet deadlines,
  clamp and output order. Synthetic replay is mandatory in the shared check suite.
- Separately registered synthetic outage drop/hold fixtures and a core integration
  test for exact deadline, release, gate-boundary and finite-horizon behavior.
- Finite terminal HOLD registration from actual converted maximum delay plus
  explicit delta/Q drain allowances, with horizon validation by the comparator.
- Canonical paired Garcia pool manifests with DL-derived phase, source hashes,
  missing-phase exclusions and strict >0.5 s measurement-gap window exclusions.
- Explicit corridor A/B propagation conversion with the registered exact
  +45.6/+106.8 ms offsets and formula components in the recipe.
- Registered M3 configured-equality semantics and documented cross-engine
  lookup, loss and release-order differences (`docs/TRACE-SEMANTICS.md`).
- Independent offline Garcia ZIP converter and configured-row comparator,
  source hashes and versioned recipes, and source-versus-configured late-run
  diagnostics (`docs/TRACE-TOOLS.md`). Synthetic Python tests run in the same
  check script used by CI and the pre-push hook; NumPy is an offline-tool
  requirement only, with no new Rust or DPDK dependency.
- HNG scheduling engine (`src/hng.rs`, `src/hng_engine.rs`, `src/hng_trace.rs`):
  per-line shared finite-Q FIFO bottleneck, fixed delay plus seeded late subset,
  monotone trace replay, C ABI, and `hng-v1` configuration
  (`docs/HNG-API.md`, `docs/HNG-DESIGN.md`).
- Engine and scheduler test suites with independent reference fixtures.
- `scripts/check.sh`, the single check suite run by the pre-push hook and CI.
  It locates cargo from `CARGO`, then `PATH`, then the rustup default, and
  fails with a clear message when none works.
- Versioned pre-push hook and installer (`scripts/git-hooks/pre-push`,
  `scripts/install-hooks.sh`).
- GitHub Actions CI running `scripts/check.sh`.
- Open-source policy scan (`scripts/check-policy.sh`) with a self-test: SPDX
  headers on owned files and rejection of environment-specific values.
- README quickstart that needs neither a NIC nor DPDK.
- M0 baseline and M1 software I/O reports in `docs/reports/`.

### Changed

- Clippy runs with `--locked`, and checks fail when they modify the working tree,
  so a stale `Cargo.lock` can no longer pass.
- Added SPDX headers to all files introduced by this fork.

### Removed

- Workstation, session and private path references from design and report documents.

### Fixed

- Preserve reference floating addition order at half-microsecond crop boundaries
  and cancel propagation offsets as integers before late-run classification.
- Preserve due trace heads for actual-time TX arbitration.
- Enforce the finite trace horizon before all intentional loss outcomes.
