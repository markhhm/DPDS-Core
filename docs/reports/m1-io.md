<!-- SPDX-License-Identifier: Apache-2.0 -->

# M1 software I/O integrity

Date: 2026-10-29. Status: software acceptance passed locally; submitted for lead
review. These results establish neither G2 timing accuracy nor physical
10G/100G throughput. M4 implementation is deferred until after this handoff.

## Revisions and scope

- Both milestone branches are `hng/m1-io`, created from `hng/core`.
- Engine baseline: DPDS-Core `e9dc33514f3526a9f588185ca1bb6fe50bcc0206`.
  This milestone changes no engine source, tests, configuration or ABI.
- Host baseline: DPDS `899b3d03d77139d6f529e6a28cd694d9ba530e32`.
- Submitted host: `6fdc1c415ba0ab2367a8fa880b42a6eca0440883`.
  Its core submodule remains pinned to the engine baseline above.
- M0 push scripts were adopted from core `ff2e33f` and host `8a4a49f`, with
  SPDX headers added forward. Neither `main` nor `hng/core` is a push target.

The changes reach the independent C oracle, synthetic fixture generators,
actual-PMD process harness, existing test lcore arguments, Meson registration,
README, CI workflow and changelog. Production RX, scheduler and TX behavior is
unchanged. The existing production worker/TX functions process the new fixtures.

## Results

| Check | Input packets | Checked output packets | Intentional Q drops | Result |
| --- | ---: | ---: | ---: | --- |
| pcap, 32 seeds | 65,536 | 60,928 | 4,608 | PASS |
| net_ring, 32 seeds, 153 epochs each | 10,027,008 | 9,321,984 | 705,024 | PASS |
| af_packet, bidirectional namespace/veth | 64 | 64 | 0 | PASS |

Every surviving pcap/ring ID, full frame, VLAN and physical egress matches the
independent expected sequence. Unexpected reordering, infrastructure packet
loss, storage drops and TX drops are zero. RX/handoff, core per-line packet and
byte counters, PMD counters, TX/receiver counts and pool reclamation reconcile.
Q drops are deliberate model outcomes, not infrastructure loss.

The complete host suite passes **14/14, 0 failed, 0 skipped**. It also passes
14/14 with four logical lcores mapped onto only two allowed physical CPU IDs.
The core passes **42/42** (9 unit, 5 engine, 28 scheduler), `cargo fmt --check`
and `cargo clippy --all-targets -- -D warnings`. The installed pre-push gates
run the same checks before branch publication; no hook bypass is used.

An independent read-only review reran the oracle, all pcap seeds, the full ring
stream and af_packet smoke. It found no C oracle/harness correctness issue.
Its private-submodule CI authentication finding was fixed by keeping recursive
checkout under `actions/checkout`'s token configuration.

## What the fixtures prove

Each pcap seed contains two 1024-packet directional inputs. Each direction
alternates synthetic VLAN 100 (cable) and VLAN 101 (LEO). These M1 fixtures use
fixed models on both VLANs: C = 8,000,000,003 bit/s, Q = 600 L2 bytes, base
delay 1,000/4,000 ns, late probability 0.5 and additional delay 15,000 ns.
The deliberately fractional rate tests rational serialization. The finite Q
produces known drops. Full frame lengths and payloads vary with seed and ID.

Capture timestamps deliberately differ from model arrival times and do not
pace replay. The RX harness injects arrival and physical ingress sequence from
the external synthetic payload. Every pcap run uses fresh input/output paths.
The production application's live RX timestamp capture is separately exercised
by af_packet; it is not replaced with pcap capture timestamps.

Ring runs one process on four lcores: source/checker, RX, scheduler and TX.
Companion net_ring ports exercise PMD TX and RX on both sides. Each seed retains
one engine and scheduler through 153 epochs. IDs, physical sequence, arrival
clock and per-line selector draw advance across epochs. The 1 ms gaps empty
payload storage; only the last epoch seals input and drains the service ledger.
Each epoch admits all arrivals before advancing the injected dequeue clock.
This bounds test memory and isolates ordering correctness. It is not continuous
offered load or a maximum-rate benchmark.

The oracle calls no Rust, DPDK or production scheduler function. It implements
its own finite L2-byte ledger, exact rational wire service, counter-based seeded
selector, and global sort by `(physical port, release time, ingress sequence)`.
Its epoch-local empty ledger is equivalent for these fixtures because each
inter-epoch gap exceeds all previous service and release times. Selector draw
offsets continue across epochs, including Q-dropped offers. Golden cases check
exact-fit Q, completion equality, fractional service, tied deadlines and legal
late inversion. Negative checks reject changed IDs, VLANs, lengths, frame bytes
and swapped output order. The initial missing-oracle and affinity-helper tests
were observed failing before implementation.

Af_packet uses two veth pairs inside a disposable user/network namespace,
without changing host interfaces or privileges. It sends only one outstanding
frame per direction and waits for both receivers before the next pair. This
keeps a low-rate smoke's physical sequence expectation valid even when a busy
runner batches RX and cable/LEO have different model delays. An earlier
two-CPU run exposed that assumption; the corrected smoke passed three repeated
two-CPU runs and the complete suite. PACKET_AUXDATA reconstructs kernel-stripped
VLANs before byte comparison. Namespace background frames are classified and
counted separately; their count varies by run and is included in conservation.

## Reproduction

Environment: x86-64 Linux/WSL2, kernel 6.18.33.2, Rust/Cargo 1.99.1, GCC 14.2.0,
Python 3.13.5, Meson 1.12.1, DPDK 26.03.0 and libpcap 1.10.5. DPDK and its
pcap development dependencies were built/extracted into user-writable prefixes.
No system packages, hugepages, WSL configuration or host privilege settings
were changed. Physical NICs and the testbed were not used.

Follow the host README's zero-NIC quickstart. Set `PKG_CONFIG_PATH`, runtime
library paths and Cargo/Meson `PATH` for your installation, then run:

```sh
# Core checkout
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked

# Host checkout, initialized pinned submodule
meson setup build
meson compile -C build
meson test -C build --print-errorlogs
./scripts/install-hooks.sh
```

The core checkout also installs its own `./scripts/install-hooks.sh` gate.
Existing build directories configured against a DPDK without pcap/ring must be
reconfigured against a build containing those PMDs. The registered tests are
`eal-affinity`, `worker`, `tx`, `lifecycle`, `reorder`, `tx_trace`, `ledger`,
`rings`, `recovery`, `null-layouts`, `io-oracle`, `io-pcap`, `io-ring` and
`io-af-packet`. Af_packet requires `unshare`, `ip`, unprivileged user/network
namespaces and namespace-local network capabilities. An unavailable prerequisite
produces Meson SKIP with its reason; successful namespace probes do not excuse
PMD, application or payload failures. No af_packet skip occurred here.

Evidence is kept with the project's private research records as the write-once archive
`2026-10-29-p1-m1-io-122851Z.tar.gz` and its SHA-256 receipt. It contains the
original build/RED/review logs, final core/host logs, the constrained-affinity
run, repeated corrected smoke logs, push-gate logs and a revision manifest.

## Open source acceptance and remaining work

- Apache-2.0 LICENSE files, upstream authors and the core pin are preserved.
  Neither baseline contains a NOTICE file to preserve. New M1 source, workflow,
  scripts and documents carry SPDX headers. Fixtures are generated synthetic
  bytes; no external dataset is redistributed.
- New defaults, fixture data and documentation contain no private environment
  values. Paths come from temporary directories, build arguments or environment.
  Comments, repository documentation, changelogs and commits are English.
- The engine remains independent of DPDK. The README documents the public test
  commands, output fields, PMD coverage, namespace prerequisites and limits.
- The CI workflow builds virtual PMDs without hugepages and invokes the same
  complete Meson suite as the push gate. **Remote CI execution is unverified.**
  While the core is private, Actions requires the documented read-only
  `DPDS_READ_TOKEN` secret for both repositories; the default workflow token
  suffices after the core becomes public. This session cannot verify secret
  availability or read remote Actions results with its connected tools.
- Existing licensing/environment debt is explicitly retained for the common M0
  cleanup: core source/tests, Cargo/build/cbindgen files, README, HNG API/design
  and the earlier core plan lack SPDX headers. Host production C/header files,
  old C tests and the devcontainer setup script also lack them. The touched host
  README and null smoke now have SPDX. The old design contains a private outbox
  reference at line 881 and machine/session-specific prose at lines 3, 5, 730,
  757, 849 and 856. No baseline CJK text was found in the scanned source/docs.
  The common OS1/OS2 CI/hook scanner is owned by M0 and is not yet part of these
  adopted M0 hook revisions. Published history was not rewritten.
  (Resolved later by the M0/M1 integration; see `integrate-m0-m1.md`.)

Differences from the task: the 10-million ring total is across 32 seeds with
bounded epochs, explicitly documented above. The M1 VLAN fixtures exercise
fixed cable/LEO line states; source trace replay/conversion acceptance belongs
to M3, and sustained software performance plus per-direction scheduler owners
belong to M4. Hosted CI and the common legacy open-source cleanup remain
unverified/pending. All required M1 software I/O checks ran locally without a
permission skip or system-setting request.
