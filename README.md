# dpds-core

## HNG fork

This fork builds on [uni-tue-kn/DPDS-Core](https://github.com/uni-tue-kn/DPDS-Core)
by Etienne Zink and the upstream DPDS project. The original Apache-2.0 license
and author metadata are retained. The proposed HNG scheduling contract and
validation plan are in [docs/HNG-DESIGN.md](docs/HNG-DESIGN.md); that document
does not claim that the design is implemented or hardware-validated.

`dpds-core` is the core of **DPDS**, a software **network-link emulator**. It
models the three effects a real network link imposes on packets — **delay**,
**rate limiting**, and **packet loss** — and exposes them through a small **C
FFI**. It is built as a `cdylib` so it can be linked into a packet-processing
data plane — DPDS runs on [DPDK](https://www.dpdk.org/) — which owns the packets
and calls into this library to decide *when* (or *whether*) each packet should be
forwarded.

The library never copies or interprets packet payloads. It only stores an opaque
packet pointer together with a computed send time, so it stays agnostic to the
data plane's packet representation.

## Pipeline

Each packet pushed into a `Buffer` passes through three stages:

| Stage       | Source           | Responsibility                                                                                                                                       |
| ----------- | ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Delayer** | `src/delayer.rs` | Samples a forwarding delay from a configurable distribution, optionally autocorrelated over time.                                                    |
| **Limiter** | `src/limiter.rs` | Enforces a rate limit, either as a **Spacer** (spreads packets out in time) or a **Token Bucket Filter** (drops packets that exceed a burst budget). |
| **Dropper** | `src/dropper.rs` | Decides whether a packet is lost, using a **Random** (Bernoulli) or **Gilbert–Elliott** (bursty) loss model.                                         |

The `Buffer` (`src/buffer.rs`) stores admitted packets ordered by their send
time. With `reordering = false` it is a FIFO `VecDeque`; with `reordering = true`
it is a min-heap keyed on send time, allowing packets whose sampled delays
reorder them to be emitted in send-time order.

### Delay correlation

When `activate_correlation` is enabled, the delayer smooths successive samples
with a rate adaptive exponential moving average (EMA). The smoothing weight
is derived from the configured `half_life_period_ns` and the measured packet
rate, so the *effective* correlation half-life stays constant in wall-clock time
regardless of how fast packets arrive.

## Quickstart (no NIC, no DPDK)

The engine needs only a stable Rust toolchain:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # if Rust is missing
git clone https://github.com/markhhm/DPDS-Core.git dpds-core
cd dpds-core
cargo build --release --locked
cargo test --locked
```

`cargo test` runs the legacy and HNG engine tests, including the scheduler
oracle fixtures. To push real packets through the engine without a physical
NIC, use the DPDS host, which embeds this repository as a submodule: its README
quickstart runs the `net_null` and `net_pcap` software ports.

## Building

Requires a stable Rust toolchain.

```sh
cargo build --release
```

This produces:

- `target/release/libdpds_core.{so,dylib,dll}` — the shared library to link against, and
- `target/libdpds_core.h` — a C header generated at build time by [`cbindgen`](https://github.com/mozilla/cbindgen) (see `build.rs` / `cbindgen.toml`).

## Using the FFI

The host first loads a configuration, creates a buffer, then pushes and pops
packets. The exported functions are:

| Function                                                                                                | Purpose                                                                   |
| ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `Config read_config(const char *path)`                                                                  | Parse a JSON config file (see below).                                     |
| `Buffer *buffer_new(Config config)`                                                                     | Allocate a buffer for the given config.                                   |
| `bool buffer_push(Buffer *self, void *packet_ptr, uint32_t packet_length_byte, double current_time_ns)` | Offer a packet. Returns `false` if it was dropped or the buffer was full. |
| `double buffer_front_time(const Buffer *self)`                                                          | Send time (ns) of the head packet.                                        |
| `void *buffer_pop(Buffer *self)`                                                                        | Remove and return the head packet pointer.                                |
| `bool buffer_is_empty(const Buffer *self)`                                                              | Whether the buffer is empty.                                              |
| `uintptr_t buffer_len(const Buffer *self)`                                                              | Number of buffered packets.                                               |
| `uintptr_t buffer_capacity(const Buffer *self)`                                                         | Configured capacity.                                                      |
| `void buffer_destroy(Buffer *ptr)`                                                                      | Free a buffer created by `buffer_new`.                                    |

A typical host loop pushes arriving packets with the current timestamp, then
forwards a packet whenever `buffer_front_time` has passed and the buffer is
non-empty:

```c
Config cfg = read_config("config.json");
Buffer *buf = buffer_new(cfg);

// on packet arrival:
if (!buffer_push(buf, pkt, pkt_len, now_ns)) {
    /* packet was dropped or buffer full — free it */
}

// in the forwarding loop:
while (!buffer_is_empty(buf) && buffer_front_time(buf) <= now_ns) {
    void *pkt = buffer_pop(buf);
    /* transmit pkt */
}

buffer_destroy(buf);
```

> **Safety:** the buffer stores raw packet pointers and does not own them. The
> caller must keep each pushed pointer valid until it is popped, must not pop or
> read the head of an empty buffer, and must use a `Buffer*` only with the
> functions above. See the `# Safety` notes on each function in the source.

## Configuration

`read_config` parses a JSON file into the `Config` struct. A full example lives
in [`config.example.json`](config.example.json):

```json
{
  "buffer_capacity": 100000,
  "reordering": false,
  "delayer": {
    "distribution": "LogNormalDistribution",
    "mean_ns": 5000000.0,
    "std_ns": 500000.0,
    "activate_correlation": true,
    "half_life_period_ns": 50000000.0
  },
  "limiter": {
    "mode": "TokenBucketFilter",
    "activate": true,
    "rate_bps": 1000000000.0,
    "limit_bit": 2000000.0
  },
  "loss_model": {
    "GilbertElliot": [0.01, 0.3, 1.0, 0.5, true]
  }
}
```

### Fields

**Top level**

| Field             | Type    | Meaning                                                                 |
| ----------------- | ------- | ----------------------------------------------------------------------- |
| `buffer_capacity` | integer | Maximum number of packets held at once; pushes beyond this are dropped. |
| `reordering`      | bool    | `false` = FIFO; `true` = emit in send-time order (min-heap).            |

**`delayer`**

| Field                  | Type   | Meaning                                                                                                                        |
| ---------------------- | ------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `distribution`         | string | One of `DeterministicDistribution`, `NormalDistribution`, `LogNormalDistribution`, `GammaDistribution`, `UniformDistribution`. |
| `mean_ns`              | float  | Mean delay in nanoseconds.                                                                                                     |
| `std_ns`               | float  | Standard deviation of the delay in nanoseconds (ignored for `DeterministicDistribution`).                                      |
| `activate_correlation` | bool   | Enable UTEMA autocorrelation of successive delays.                                                                             |
| `half_life_period_ns`  | float  | Correlation half-life in nanoseconds (only used when correlation is active).                                                   |

**`limiter`**

| Field       | Type   | Meaning                                                                         |
| ----------- | ------ | ------------------------------------------------------------------------------- |
| `mode`      | string | `Spacer` or `TokenBucketFilter`.                                                |
| `activate`  | bool   | Enable rate limiting. When `false` the limiter is a no-op.                      |
| `rate_bps`  | float  | Target rate in bits per second.                                                 |
| `limit_bit` | float  | Spacer: max in-flight backlog in bits. TBF: bucket size (burst budget) in bits. |

**`loss_model`** — a tagged union, one of:

- `{ "Random": p }` — each packet is dropped independently with probability `p`.
- `{ "GilbertElliot": [p, r, k, h, state_is_good] }` — bursty loss with a Good/Bad
  state machine:
  - `p` — Good → Bad transition probability
  - `r` — Bad → Good transition probability
  - `k` — probability of correct transmission while in the Good state
  - `h` — probability of correct transmission while in the Bad state
  - `state_is_good` — initial state flag (always reset to `true`/Good on startup)

## HNG implementation

The HNG engine, C ABI, VLAN-line configuration and normalized trace schema are
documented in [docs/HNG-API.md](docs/HNG-API.md). Its contract is
[docs/HNG-DESIGN.md](docs/HNG-DESIGN.md). The upstream APIs above remain available.

The [trace pipeline inventory](docs/reports/m3-trace-inventory.md) records source
contracts and unresolved replay semantics. Garcia configured-interval conversion
is available; other source adapters and end-to-end replay acceptance are pending.

The offline [Garcia trace tools](docs/TRACE-TOOLS.md) convert a selected raw ZIP
window to `hng-trace-v1`, compare configured intervals with TheaterQ CSV rows,
and report source-versus-configured late-run distributions. A synthetic quick
start requires only Python and NumPy, without DPDK or a NIC. These tools do not
certify cross-engine packet deadlines, outage reconstruction or hardware timing.

## Hardware planning

The standalone library needs no NIC, DPDK or hugepages. Real-packet memory
requirements come from the matching DPDS host's pool calculator.
Set `DPDS_SRC` to an absolute host checkout path and build its performance tools first.
Keep its DPDK runtime library path available.

```sh
"$DPDS_SRC/build/hng-pool-calc" --lines-per-port 1
"$DPDS_SRC/build/hng-pool-calc" --lines-per-port 2
```

Defaults describe two ingress pools.
They use 100 Gbit/s, t=142 ms, delta=320 ms,
Q=50 ms, 1,522-byte L2 frames, seven lcores and 2 MiB pages.
Early budgets were **20 GiB minimum / 40 GiB recommended**.
They rounded 19.432 / 38.826 GiB of pools for one / two lines per direction.
The [earlier host](https://github.com/markhhm/DPDS/tree/749d5543595bcd8fd2fd53eb0dd15fd1fc1c2182)
used a 2,496-byte stride. Current host observer metadata gives a 2,624-byte
stride and approximately 20.43 / 40.81 GiB for those same pools.

The pools alone now need at least 21 / 41 whole GiB, plus EAL headroom.
Reserve separate ordinary RAM for core and observer metadata.
Treat 20 / 40 GiB as historical budgets, not sufficient current reservations.
Recompute for the actual frame, trace, port and NUMA layout. These estimates
do not qualify a physical allocation or sustained throughput.

## How to check accuracy (software-only)

Use the NumPy environment described under [checks](#checks-pre-push-hook-and-ci).
Set `PYTHON` to its absolute interpreter path. All commands here use software
fixtures or virtual scheduler time. They do not establish wire precision or G2.

### M2: injected faults in the host reporter

```sh
(cd "$DPDS_SRC" && prlimit --as=4294967296 -- "$PYTHON" -m unittest \
  tools.g2.tests.test_report tools.g2.tests.test_protocol tools.g2.tests.test_calibration)
```

The host owns these independent checks. They inject packet order/identity,
timestamp, capture-gap and calibration errors and assert their required verdicts.

### M3: a registered canonical replay

Run from this repository root. Set `RAW` to downloaded Garcia ZIPs and `OUT`
to a new external evidence directory. This checks one registered 11 s UL
corridor-A case with the adopted 0.01 ms converter.

```sh
mkdir -p "$OUT"
CARGO_BUILD_JOBS=1 prlimit --as=4294967296 -- cargo build --locked --release --bin trace_replay
prlimit --as=4294967296 -- "$PYTHON" scripts/garcia_trace.py \
  "$RAW/2025-04-16-09-26-16__ul.csv.zip" "$OUT/canonical.json" \
  --start-ns 38000000000 --duration-ns 11000000000 --corridor A --seed 1 \
  --queue-drain-ns 1000000 --pool docs/reports/m3-canonical-pool-11s.json --pool-index 8
prlimit --as=4294967296 -- "$PYTHON" scripts/replay_trace.py \
  "$OUT/canonical.json" --source "$RAW/2025-04-16-09-26-16__ul.csv.zip" \
  --runner target/release/trace_replay --output "$OUT/replay.json"
```

Inspect oracle mismatches and `realized_lateness`. One case is not the full
canonical verdict. The [complete report](docs/reports/m3-canonical.md) retains
56 cases: 24 PASS, 0 FAIL and 32 INCONCLUSIVE, including final-core and pcap checks.
Keep insufficient windows and use the registered method for larger profiles.

### M4: performance and allocation

Use the existing microbenchmark command below and host calculator commands
above. Timings describe CPU execution. Pool outputs describe reservations.
Neither establishes physical timing accuracy.

## Performance microbenchmarks

`examples/hng_bench.rs` measures the scheduling core without DPDK or a NIC:

```sh
CARGO_BUILD_JOBS=1 prlimit --as=4294967296 -- cargo run --release --locked --example hng_bench -- --mode both --repeat 3
```

Each sample times one batch (default 1024 packets) of admission plus every
dequeue that is due, and divides by the batch size. Arrivals are back-to-back
frames at `--c-bps` (default 100 Gbit/s, 1518-byte mbufs), so in steady state
each packet is admitted and dequeued once. Batches that end before
`--warmup-ns` (default t + delta = 462 ms of virtual time) are not sampled.
Fixed mode uses `--t-ns`, `--delta-ns` and `--p` (defaults 142 ms, 320 ms,
0.01). Trace mode replays a synthetic sawtooth trace between t and t + delta
with one row per `--trace-row-ns`. The output reports p50, p99 and max
ns/packet, the CPU model, the `/proc/cpuinfo` MHz value and the kernel clock
source. The `arithmetic` lines compare the exact u128 service-time arithmetic
with a rounded u64 picosecond baseline that the core does not use because it
drifts.

These numbers describe the machine they ran on. They are not evidence of
sustained 100 Gbit/s operation. Results and rerun commands are in
[docs/reports/m4-perf.md](docs/reports/m4-perf.md).

## License

Licensed under the [Apache License, Version 2.0](LICENSE).

## Checks, pre-push hook and CI

`scripts/check.sh` is the complete check suite. It runs the open-source policy
scan (`scripts/check-policy.sh`: SPDX headers and environment-specific values
such as lab hostnames, personal paths or private IP addresses) and its
self-test, `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
`cargo test --locked`, builds the offline replay executable, and runs the Python
synthetic/integration tests. It then fails if any check modified the working tree
(for example a stale `Cargo.lock`). GitHub Actions (`.github/workflows/ci.yml`)
and the pre-push hook run this same script.

The full check suite requires Python 3.11+ and NumPy 2.x for offline trace tests.
The Rust engine itself still needs only Rust. Use a virtual environment:

```sh
python3 -m venv .venv
.venv/bin/python -m pip install -r scripts/trace-requirements.txt
PYTHON=.venv/bin/python scripts/check.sh
```

Set `PYTHON` to the same interpreter when pushing so the hook uses it.
If NumPy cannot be imported, `check.sh` exits nonzero with installation help;
it never skips the trace suite into a passing check. Missing measured sources
skip only the explicitly external-data test, as described in
[trace replay](docs/TRACE-TOOLS.md#actual-scheduler-replay-and-independent-oracle).

Install the versioned hook after cloning:

```sh
./scripts/install-hooks.sh
```

The hook checks a clean checkout of the exact commit being pushed. Check out each
commit before pushing another branch. Deletions need no build. A check failure
blocks the push; fix the failure or the missing tool. Never bypass hooks. The
installer refuses to replace a different existing hook.

Upstream files that never carried an SPDX header are listed in
`scripts/spdx-exempt.txt`; new files must carry one. Any accepted policy-scan
hit must be listed explicitly in `scripts/env-allow.txt`.
