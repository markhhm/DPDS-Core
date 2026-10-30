# P1 M0 / M2 execution plan

**Goal:** Independently reproduce M0 and enforce checks before starting the
independent M2 reference/reporting tools. Task authority: research records
`research/agents/tasks/2026-10-29-p1-m0-m2-obs.md` and the M0/M2 roadmap.

**Architecture:** Repository-owned, installed pre-push scripts run existing
checks. M2 lives in host `tools/g2/`, uses Python Fraction and its own selector,
and consumes complete ingress/receiver streams. It never imports Rust model
logic. Observer wiring follows worker's forthcoming specification.

**Constraints:** Only own clones/branches; no testbed, shared branch pushes,
hook bypass, hardware timing or throughput claims. Preserve existing APIs,
fixtures and submodule pin during M0. Raw logs are append-only in own outbox.

## Scope / review focus

- Hook scope: both README files, `scripts/install-hooks.sh`,
  `scripts/git-hooks/pre-push`; clean tree, exact pushed HEAD, deletion and
  environment errors must fail safely. Existing hooks must not be overwritten.
- M2 scope: `tools/g2/{reference,report}.py`, tests and CLI docs; a core-output
  harness for at least 1,000,000 event comparisons. No scheduler behavior change.
- Check fractional C, exact-fit Q, trace boundaries/loss/outage, frontier/end
  completeness, identity gaps/duplicates, sample bias and clock uncertainty.
- Keep all four timing/order/storage gates and all loss layers visible.
- Task inconsistency: adding 0.2 ms to an ideal run cannot exceed the fixed
  0.5 ms p99 limit. Retain thresholds and explicitly test this; also inject
  0.2 ms over a registered 0.4 ms tail to demonstrate p99 failure.

## Steps / ledger

- [x] Reproduce core e9dc335 and host 899b3d0 in clean clones: 42/42 and 9/9.
- [x] RED: both missing hook installers fail before implementation.
- [x] Add minimal installers/hooks; run actual deliberate-failure commits
  locally, verify rejection, discard only these authorized commits, then green.
- [ ] Commit M0 reports with log paths; push only `hng/m0-hooks` on each fork.
  Record M0 in own research records branch and lab notebook for lead integration.
- [ ] Read Stage 1 report/probe contract and current M2 specification. Write
  failing oracle tests, then implement streaming reference and core harness.
- [ ] Write failing reporter injection/coverage tests, then implement reporter.
- [ ] Compare at least 10^6 events, run all relevant checks, record limits and
  pending observer specification; push `hng/m2-obs` only after hooks pass.

Implementation follows the supplied task directly in this session. integration lead
owns review/integration; research records main completion remains the lead's gate.
