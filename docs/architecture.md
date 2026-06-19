# Chronos Architecture

Chronos is an HTTP-backend time synchronization gateway. It lets a restricted
data center that may only egress over HTTPS still discipline its clocks: a
`chronos-gateway` samples time from a `chronos-server` over HTTP/HTTPS and feeds
the samples to a local `chronyd` through chrony's SOCK refclock. `chronyd` then
disciplines the gateway host clock and serves NTP to the internal network.

```text
chronos-server (HTTP Time API)
        | HTTP / HTTPS
        v
chronos-gateway (sample, filter, estimate offset)
        | Unix datagram socket (chrony SOCK refclock)
        v
chronyd (disciplines local clock, serves NTP)
        | UDP/123 inside the DC
        v
internal servers (chrony / timesyncd / ntpsec)
```

## Clean Architecture

The codebase follows the project's Clean Architecture standard
([`docs/standards/architecture.md`](standards/architecture.md)): source
dependencies point inward only, and the dependency edges are enforced at compile
time by the Cargo workspace.

```text
crates/
  chronos-core     # domain: types, ports (traits), pure algorithms — no I/O
  chronos-chrony   # adapter: implements OutputBackend over a Unix datagram socket
  chronos-server   # composition root: axum HTTP/HTTPS API + chronyc status provider
  chronos-gateway  # composition root: reqwest client, sampler, scheduler, status API
  chronos-ntp      # reserved placeholder for v2 backends
```

Dependency arrows (all inward):

```text
chronos-chrony  -> chronos-core
chronos-server  -> chronos-core
chronos-gateway -> chronos-core, chronos-chrony
```

### `chronos-core` (innermost)

Owns the data that crosses boundaries and the trait contracts (ports) that outer
crates implement. It depends only on `serde`/`serde_json`/`thiserror` — never on
`tokio`, `axum`, `reqwest`, or chrony.

- Types: `TimeSample`, `SampleQuality`, `BackendStatus`, `TimeStatus`,
  `SyncState`, `TimeProvider`, `GatewayState`.
- Ports: `OutputBackend`, `TimeStatusProvider`, `MonotonicClock`, `WallClock`.
- Errors: `ChronosError` (a concrete `thiserror` enum).
- Pure logic: round-trip-time, offset estimation, median, outlier detection, and
  the backend transport `SecurityPolicy`.

Because policy never depends on details, the domain logic is unit-testable with
no runtime, clock, filesystem, or network.

### `chronos-chrony` (adapter/driver)

Implements the `OutputBackend` port by encoding a `TimeSample` into chrony's
40-byte `sock_sample` wire format and sending it over a
`std::os::unix::net::UnixDatagram`. It performs no clock adjustment of its own.
See [`chrony-integration.md`](chrony-integration.md).

### `chronos-server` (composition root)

An `axum` application exposing `/time`, `/healthz`, and `/status`. It supports
three transport modes (native HTTP, native HTTPS via `axum-server` + `rustls`,
and HTTP behind a reverse proxy). Synchronization status comes from a
`TimeStatusProvider` — the v1 implementation shells out to `chronyc tracking`.
See [`deployment-server.md`](deployment-server.md).

### `chronos-gateway` (composition root)

A `tokio` application that builds one `reqwest` client per backend (enforcing the
transport security policy first), collects burst samples, filters them, selects
the median offset, and writes the chosen sample to the chrony backend. It exposes
a local `/healthz` and `/status` endpoint. See
[`deployment-gateway.md`](deployment-gateway.md).

### `chronos-ntp` (v2 placeholder)

Empty crate reserved for v2 output backends (`builtin_ntp_server`,
`direct_clock_setter`). v2 adds new `OutputBackend` implementations without
rewriting the v1 core.

## Timestamps

All domain timestamps are `i128` nanoseconds. Unix-nanosecond arithmetic stays
exact and signed (offsets may be negative) without overflow. Conversion to/from
OS clock types happens only at the edges (server `SystemTime`, gateway `Instant`
for round-trip time, chrony `timeval` in the writer).
