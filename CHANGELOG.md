<!-- SPDX-License-Identifier: Apache-2.0 -->

# Changelog

No HNG release exists yet. All validation is software-only.
Accuracy at 100G remains unverified. Host integration entries link to DPDS PRs.

## [Unreleased]

### Added

- M1–M4: provide the HNG C ABI and reduce scheduler ledger memory. Validation: software-only. ([#2](https://github.com/markhhm/DPDS-Core/pull/2), [DPDS #4](https://github.com/markhhm/DPDS/pull/4))
- M2: add the independent oracle, counter checks, and G2 reporter. Validation: software-only. ([#3](https://github.com/markhhm/DPDS-Core/pull/3))
- M3: adopt Decision F, 0.01 ms subdivision, and canonical replay evidence. Validation: software-only. ([#4](https://github.com/markhhm/DPDS-Core/pull/4), [#5](https://github.com/markhhm/DPDS-Core/pull/5), [#7](https://github.com/markhhm/DPDS-Core/pull/7))
- M6 host integration: report matrices sequentially with pcapng mode and verified resume. Validation: software-only. ([DPDS #7](https://github.com/markhhm/DPDS/pull/7), [DPDS #16](https://github.com/markhhm/DPDS/pull/16), [DPDS #20](https://github.com/markhhm/DPDS/pull/20))
- 7-lcore observation: record native v2 ownership and per-direction snapshots. Validation: software-only. ([#6](https://github.com/markhhm/DPDS-Core/pull/6), [DPDS #10](https://github.com/markhhm/DPDS/pull/10))
- pcapng host integration: convert same-clock nanosecond captures into G2 event streams. Validation: software-only. ([DPDS #13](https://github.com/markhhm/DPDS/pull/13), [DPDS #16](https://github.com/markhhm/DPDS/pull/16))
