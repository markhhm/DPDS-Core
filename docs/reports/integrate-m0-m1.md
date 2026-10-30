<!-- SPDX-License-Identifier: Apache-2.0 -->

# M0 + M1 integration and review fixes

Date: 2026-10-29. Software-only on a development workstation; no testbed access.
Branch `hng/integrate-m0-m1` in both forks, based on `hng/core`
(core `e9dc335`, host `899b3d0`), ready to fast-forward `hng/core`.
No push to `hng/core` or `main`; no hook was bypassed.

## Merge

- M0 (`hng/m0-hooks`: core `ba628ef`, host `8a4a49f`) merged first, then M1
  (`hng/m1-io`: core `a4bf8b6`, host `6fdc1c4`). Authors' commits are kept.
- Conflicts were add/add on `scripts/git-hooks/pre-push`, `scripts/install-hooks.sh`
  (both repos) and `docs/reports/m0-m2-plan.md` (core). Resolution: M1's
  versions (M0 content plus SPDX headers) with M0's completed checklist item.
  README files merged without conflict.

## Commits after the merges

Core: `d411118` policy scan + self-test; `481d142` single `scripts/check.sh` for
hook and CI; `05dcf7a` SPDX headers and environment-reference removal; `38a2393`
CHANGELOG and quickstart; `6fb1e18` self-test wording fix; `91909c5` robust cargo
location; then this report.

Host: `5cba183` stepped-clock ring test; `80aae6a` single `scripts/check.sh`, CI,
SKIP accounting, core pin `91909c5`; `a5cc3b5` SPDX headers, CHANGELOG, README.
The host pin is `91909c5`; the only later core commit is this report.

## Review items

| Item | Resolution |
| --- | --- |
| Core hook passes a stale `Cargo.lock` | `scripts/check.sh` runs `cargo clippy --locked` and fails when any check leaves the working tree modified. |
| No CI in core; host CI lacks core checks | `.github/workflows/ci.yml` in both repos runs `scripts/check.sh`, the same script the hook runs. Host `check.sh` runs the pinned core's `check.sh` (policy, fmt, clippy, tests) before the Meson suite. |
| af_packet may SKIP silently in CI | `scripts/check-skips.py` prints every Meson SKIP, fails unless the test is in `DPDS_ALLOW_SKIP`, and in Actions adds a warning annotation and a step-summary line. CI allows only `io-af-packet`. |
| No OS2 environment scan / OS1 header check | `scripts/check-policy.sh` (core; the host calls it through the submodule) with `scripts/check-policy-test.sh`. Upstream exemptions: `scripts/spdx-exempt.txt` (upstream initial-commit files that never had a header). Comment-less data formats (`*.json`, `*.csv`, `*.lock`, `LICENSE`) are excluded by rule. Allowlist: `scripts/env-allow.txt`, exact `path:text` entries; core has one (the rustup default in `find-cargo.sh`), host has one (the upstream devcontainer's `CARGO_HOME` default). |
| M0 files without SPDX | Headers on all fork-owned files in both repos, including the M0 files, HNG sources, tests and documents. |
| Internal paths, hosts and session names in docs | Removed from `HNG-DESIGN.md`, `m0-baseline.md`, `m0-m2-plan.md`, `m1-io.md` and the earlier core plan; replaced by generic roles. The scan finds no remaining hits, and no CJK text was found. |
| No CHANGELOG / quickstart | `CHANGELOG.md` (Keep a Changelog, Unreleased) in both repos. Core README: quickstart with no NIC or DPDK. Host README: clone-to-first-run quickstart on `net_null`/`net_pcap`. Upstream attribution kept. |
| Ring test has no time dimension | New `io-ring-stepped`, described below. Batched `io-ring` unchanged. |
| M0/M1 branch conflict | Resolved by this integration (see Merge). |
| Hook messages use internal wording ("report to the lead") | Replaced by neutral wording. |

Additional lead requirement: `scripts/find-cargo.sh` (sourced by both check
scripts) uses `CARGO`, then `PATH`, then the rustup default directory, and
puts that directory on `PATH` for Meson. When none of these gives a working
cargo, it exits with a clear message.

## Stepped-clock ring variant

`io-ring-stepped` runs the same 32 seeds × 153 epochs and the same oracle as
`io-ring`. The fixture clock trails the newest consumed arrival by 0–4 µs, so
TX output interleaves with arrivals, and the scheduler chooses among line
heads where only some are due, under the input watermark. Source, sink and
handoff rings have 64 entries. Before each epoch the source rings fill until a
TX burst returns short, and the scheduler waits until the RX producer has
blocked on a full handoff ring. Both events are asserted for each seed. The TX
owner stops dequeuing before the bounded TX backlog would fill. This hold
never triggered in the local run (`tx_hold=0`).

Local per-seed counters (seed 0): 21,622 interleaved steps, 21,622
partially-due steps, 345 full source rings, about 3.1 M blocked RX-handoff
enqueue attempts. Totals equal `io-ring`: input 10,027,008, output 9,321,984,
Q drop 705,024. Every ID, frame and order matched the oracle.

Mutation checks (reverted afterwards):

- Removing the worker's watermark completeness check (`if (!complete) continue;`):
  `io-ring` **passes**, `io-ring-stepped` **fails**.
- Inverting the TX tie-break on `ingress_seq`: both fail.

## Check-script evidence (red, then green)

These checks ran on disposable local clones. The hook was called with Git's
pre-push stdin, without a real push.

| Input | Old hook | New hook |
| --- | --- | --- |
| Core: `Cargo.toml` version bumped, lock not updated | exit 0, leaves `Cargo.lock` modified | exit 101 (`--locked` refuses the update), tree clean |
| Core: committed doc naming a lab host | not checked | exit 1, OS2 hit |
| Core: new `.rs` without SPDX | not checked | exit 1, OS1 hit |
| Core: unmodified integration tip | — | exit 0 |
| Core: `check.sh` with a modified tracked file | — | exit 1, dirty-tree message |
| Host: committed doc naming a lab host | exit 0 | exit 1, OS2 hit |
| Host: new `.c` without SPDX | not checked | exit 1, OS1 hit |
| Skip gate, synthetic testlog with an af_packet SKIP | — | exit 1 with no allowance; exit 0 with `DPDS_ALLOW_SKIP=io-af-packet`; exit 1 when another test is allowed |
| `find-cargo.sh`: no cargo on `PATH` or in the home default | — | exit 1, "cargo not found" |
| `find-cargo.sh`: invalid `CARGO` | — | exit 1, "not a working cargo" |
| `find-cargo.sh`: cargo only in the rustup default | — | found; tests ran |

The policy self-test also covers each pattern class, the exemption and allowlist
behaviour, and numbers that are not addresses. It has 20 cases, all passing.

## Tests run

- Core `scripts/check.sh` at `91909c5`: policy and self-test (20/20); fmt; clippy
  `--locked -D warnings`; `cargo test --locked` **42 passed** (9 + 5 + 28), 0 failed.
- Host `scripts/check.sh` at `a5cc3b5` from a clean build directory: the pinned
  core checks as above, then Meson **15/15 OK, 0 skipped**. The 15 tests are
  14 existing ones and the new `io-ring-stepped`.
- Both pushes ran the hook already installed in the shared clones (the earlier
  M0/M1 version): core fmt, clippy and tests passed; host Meson 15/15 OK.
  That hook was not reinstalled, so that other work branches, which do not
  have `scripts/check.sh`, can still push. `install-hooks.sh` installs the new
  hook in fresh clones.

## CI status

**Unverified.** This workstation has no GitHub CLI or API access to
read Actions results. The host workflow needs the documented `DPDS_READ_TOKEN`
secret while the core is private, and it is not known whether the secret
exists.

## Deviations and open items

- The host pin is `91909c5`, not the core tip; the only difference is this report.
- Upstream files keep no SPDX header and are listed explicitly, as allowed.
- The DPDK tarball checksum and SHA-pinned actions were not added. They are
  review minor items.
- Branch protection with a required status check has to be configured on
  GitHub by the repository owner.
- `docs/superpowers/plans/` (agent workflow plans) remained at the time of this
  report, with environment references removed; it was later removed from the fork.
- None of these results are precision, throughput or G2 evidence.
