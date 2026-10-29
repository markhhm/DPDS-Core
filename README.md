<!-- SPDX-License-Identifier: Apache-2.0 -->

# dpds-core

## HNG fork

This fork builds on [uni-tue-kn/DPDS-Core](https://github.com/uni-tue-kn/DPDS-Core)
by Etienne Zink and the upstream DPDS project. The original Apache-2.0 license
and author metadata are retained. The proposed HNG scheduling contract and
validation plan are in [docs/HNG-DESIGN.md](docs/HNG-DESIGN.md); that document
does not claim that the design is implemented or hardware-validated.

### Hardware and validation requirements

The Rust scheduling library does not require DPDK, physical NICs or hugepages.
The host owns DPDK I/O. Functional acceptance uses fake-clock tests and the
`net_null`, `net_ring`, pcap and `af_packet` virtual devices before physical
accuracy or throughput qualification.

For the proposed bidirectional 100GbE qualification, reserve two data ports
with a Gen4 x16 adapter or two independent Gen3 x16 adapters, eight physical
data-plane cores plus control/observer cores, and at least 64 GiB RAM including
32 GiB of node-local hugepages. Keep NICs, host cores and packet memory on the
same NUMA node where possible. These are workload-dependent estimates; the
inspected host supports at most four lcores, and additional cores do not
automatically increase throughput. Neither ConnectX-4 nor ConnectX-5 is
mandatory.

The candidate host stack is Ubuntu Server 24.04 LTS with kernel 6.8 and DPDK
26.03, subject to qualification of the exact NIC, firmware and driver stack.
An independent traffic source and calibrated ingress/egress timestamps in one
clock domain are required for physical acceptance. The targets are median
absolute delay error <= 0.1 ms and p99 <= 0.5 ms; no physical precision or
100G throughput has been validated by this design branch. See
[MACHINE-REQUEST.md](docs/MACHINE-REQUEST.md) for supported NIC candidates,
memory calculations, software gates, driver constraints and the measurement
procedure. Supply machine-specific values through private configuration or
parameters, rather than copying deployment values into source or defaults.

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

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
