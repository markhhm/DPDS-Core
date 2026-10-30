# M0 baseline and pre-push verification — observation worker

2026-10-29. Software-only on development host/WSL2. No testbed access.

Fresh network clones reproduced DPDS-Core
`e9dc33514f3526a9f588185ca1bb6fe50bcc0206` and host
`899b3d03d77139d6f529e6a28cd694d9ba530e32`; host gitlink was exactly that core.
No baseline code was changed or repaired.

| Check | Result |
| --- | --- |
| `cargo fmt --check` | exit 0 |
| `cargo clippy --all-targets -- -D warnings` | exit 0 |
| `cargo test --locked` | 42 passed (9 unit + 5 engine + 28 scheduler), 0 failed |
| `meson setup build` / `meson compile -C build` | exit 0, pinned core built |
| `meson test -C build --print-errorlogs` | 9 passed, 0 failed |

Environment is exactly the exports in the implementation worker report: Cargo from
`~/.cargo/bin`, Meson/Ninja from `~/opt/hng-build/bin`, DPDK pkg-config and
runtime library paths under `~/opt/dpdk-26.03/lib/x86_64-linux-gnu`.

## Hooks

Core hook implementation: `ff2e33f`; host: `8a4a49f`, both on `hng/m0-hooks`.
`./scripts/install-hooks.sh` installs the versioned hook and refuses to overwrite
another hook. A push requires clean tracked/untracked state and its commit
must be the checked-out HEAD. Deletion-only updates require no build.
Core runs fmt, clippy and locked tests. Host sets up its build if necessary,
then compiles the pinned submodule and runs all Meson tests. Missing tools and
failed checks stop the hook; no bypass was used.

Actual deliberate-failure commits, **never pushed**, were:

- Core `ebc804d97428241e6eb5f9ef9ffb502c5853a65e`: failing Rust assertion;
  installed hook exited 101 and recorded `M0 deliberate failure`.
- Host `474054314218fa8a09906615c9dd12ad31118cfa`: failing C assertion in
  reorder test; installed hook exited 1 and recorded `M0 deliberate failure`.

Both disposable commits were discarded as the task requested, followed by a
successful full hook run at the restored HEAD. Deletion, different pushed HEAD,
dirty checkout and idempotent installation also passed local acceptance checks.
Missing installers were exercised before implementation (exit 127 in both repos).

## Evidence and integration

Full original logs (append-only, no files overwritten):

- `<shared-outbox>/2026-10-29-m0-baseline-115815Z/`
- `<shared-outbox>/2026-10-29-m0-hooks-115945Z/`

implementation worker's handoff report is already present in research records origin/main
(`11eb2eb` introduced it). This report and hook branches are submitted for
**integration lead review**, with no push to main or hng/core. The lead owns integration.
M2 can now proceed on separate `hng/m2-obs` branches after M0 pushes succeed.

Deviations: none in baseline or checks. Hook validation invokes installed hooks
with Git's actual ref-update stdin, without performing a test push, as requested.
No precision, throughput, NIC timestamp or hardware G2 result was measured.
