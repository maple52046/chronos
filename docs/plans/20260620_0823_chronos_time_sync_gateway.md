# Chronos HTTP-Backend Time Synchronization Gateway — Consolidated Plan

## 1. Purpose

Define and build **Chronos**, an HTTP-backend time synchronization gateway that
lets network-restricted data centers obtain accurate time over HTTPS (instead of
outbound NTP/UDP 123), discipline a local `chronyd`, and serve NTP internally.
This document consolidates the v1 product develop plan together with the durable
deployment constraints surfaced while preparing the first rollout.

## 2. Source Scope

Consolidated from the manuscripts under `docs/plans/manuscripts/`:

- The full v1 product develop plan (architecture, crates, API, config, chrony
  integration, Docker, systemd, milestones, testing, definition of done).
- A first-deployment plan that exercised the product end to end; only its
  reusable findings (environment constraints and config patterns) are retained
  here, not the site-specific operational log.

`README.md` (the consolidation spec) is excluded as a source.

## 3. Consolidated Background

Some data centers permit only outbound HTTPS (verified cert + domain) and SSH to
internal hosts, while blocking outbound NTP/UDP 123, arbitrary UDP, and direct
public time sync. Chronos bridges this gap:

```text
External / reachable zone:   chronos-server  (HTTP Time API)
Restricted DC:               chronos-gateway + chronyd
Internal servers:            existing NTP clients -> chronos-gateway
```

`chronos-gateway` polls `chronos-server` over HTTP(S), estimates the clock
offset from round-trip-timed samples, and feeds good samples into a host
`chronyd` via a SOCK refclock. `chronyd` disciplines the gateway clock and
serves NTP to the internal network. v1 has been implemented (crates, server,
gateway, chrony writer, packaging, docs) and is moving toward production use.

## 4. Confirmed Decisions

- Two components: `chronos-server` (HTTP Time API) and `chronos-gateway`
  (sampler that writes to chrony's SOCK refclock). The gateway never adjusts the
  clock directly (no `clock_settime`); `chronyd` owns discipline.
- Language/stack: Rust workspace; `axum` + `tokio` server; `reqwest` (rustls-tls)
  gateway client; `serde` + `serde_yaml` config; `clap` CLI (`--config`,
  `--help`); `tracing` + `tracing-subscriber` (json/text); `rustls` for TLS; a
  `std` Unix datagram socket for the chrony SOCK refclock.
- Clean Architecture: `chronos-core` is the dependency-free domain + ports;
  `chronos-chrony` implements the `OutputBackend` port; `chronos-server` /
  `chronos-gateway` are composition roots; `chronos-ntp` is a v2 placeholder.
- Domain timestamps are `i128` nanoseconds.
- Server transport modes: native HTTP, native HTTPS, and HTTP behind a reverse
  proxy (Nginx/Caddy/HAProxy; reverse proxy is the recommended production mode).
- Docker: a **single combined image** containing both binaries, no fixed
  `ENTRYPOINT` (consumer picks via `command`/CMD), no baked `HEALTHCHECK`
  (server `:8080` vs gateway `:9090` differ; healthchecks live per service in
  Compose). Image reference `ghcr.io/maple52046/chronos:${version}-${ts}` with
  `ts` = UTC `YYYYMMDDHHMMSS`. (Version-string scheme — see Open Questions.)

## 5. Architecture and Design Principles

- Clean Architecture dependency rule: source dependencies point inward only;
  the domain has no async runtime, HTTP, chrony, or I/O. Cross-layer work goes
  through traits (ports) implemented at the edges and wired at the composition
  root.

```text
chronos-core (domain + ports)   <- chronos-chrony (impl OutputBackend)
        ^                        <- chronos-server (axum HTTP/HTTPS + chronyc)
        |                        <- chronos-gateway (reqwest, sampler, status)
chronos-ntp (v2 placeholder)
```

- Ports defined in core: `OutputBackend`, `TimeStatusProvider`, monotonic/wall
  clock traits. Outer crates implement them; the core never names a concrete
  adapter.
- Gateway data flow per round: monotonic `t0` -> `GET /time` -> monotonic `t3`;
  `rtt = t3 - t0`; `estimated_server_time = server_send + rtt/2`;
  `offset = estimated_server_time - local_wall_time`. Collect a burst, filter,
  pick the median-offset sample, write it to chrony.

## 6. Functional Scope

In scope (v1):

- `chronos-server`: `/time`, `/healthz`, `/status`; v1 JSON; `Cache-Control:
  no-store`; HTTP + native HTTPS + reverse-proxy modes; `chronyc tracking`-based
  time status provider.
- `chronos-gateway`: HTTP/HTTPS backend client with TLS validation; transport
  security policy; burst sampling; monotonic RTT; sample filtering (high-RTT,
  unsynchronized, outliers, insufficient samples); offset estimation; chrony
  SOCK refclock writer; local `/healthz` + `/status`.
- chrony integration: `chronyd` on the gateway host consumes the `CHRO`
  refclock, disciplines the gateway clock, and serves NTP to the DC.
- Deployment: single Docker image, Compose examples, systemd units,
  `tmpfiles.d`, Nginx example, Ubuntu/Debian setup guide.
- Reliability: retry/backoff, multi-backend failover, degraded state, chrony
  socket + backend-unreachable handling.
- Security: backend allowlist, TLS enforcement, optional SPKI pinning, Nginx
  hardening guide.

## 7. Constraints and Rules

- Gateway never calls `clock_settime`; only `chronyd` disciplines the clock.
- Reject a sample on: HTTP failure, TLS failure, invalid JSON, backend
  unsynchronized, RTT > `max_rtt_ms`, outlier, or fewer than `min_good_samples`
  in a round.
- Transport security policy: validate cert + hostname for `https://`; allow
  `http://` loopback when `allow_plain_http_loopback`; reject remote `http://`
  unless `allow_plain_http_backends`.
- Repo standards: Clean Architecture dependency rule; rustfmt default + clippy
  `-D warnings`; `thiserror` in libs, `anyhow` in binaries; `///` rustdoc on
  public items; comment-content rule. Plans live under `docs/plans/manuscripts/`
  as `YYYYMMDD-<short-topic>.md`.
- Operational constraints learned from deployment (generalizable):
  - The runtime image runs as a non-root `chronos` user, so the container
    cannot bind privileged ports (e.g. `:80`); publish a host port mapping to a
    high in-container port instead.
  - The runtime image intentionally omits `chronyc`; without a chrony source
    reachable by the server, `chronos-server` reports `sync: unknown`, and the
    gateway then rejects every sample. End-to-end sync requires chrony at the
    server's time source and `chronyd` + SOCK refclock on the gateway host.
  - Privileged gateway-host setup (creating `/run/chronos`, installing and
    configuring chrony, wiring the SOCK refclock) may require sudo; plan for
    hosts where sudo is password-gated.

## 8. Data Model and Format Notes

- `TimeSample { backend_name, server_send_unix_nanos: i128,
  local_receive_unix_nanos: i128, rtt_nanos: u64, estimated_offset_nanos: i128,
  quality: SampleQuality }`.
- `SampleQuality`: `Good | HttpError | TlsError | InvalidResponse |
  BackendUnsynchronized | HighLatency | Outlier | InsufficientSamples`.
- `BackendStatus`: `Synchronized { stratum: Option<u8>, last_offset_nanos:
  Option<i128> } | Unsynchronized | Unknown`.
- `GatewayState`: `Starting | Sampling | Synchronized | Degraded |
  Unsynchronized`.
- chrony SOCK refclock wire format: 40-byte `sock_sample` =
  `timeval { tv_sec, tv_usec }` (16B) + `double offset` (8B) + `int pulse` (4B) +
  `int leap` (4B) + `int _pad` (4B) + `int magic = 0x534F434B` (4B), sent over a
  connected `UnixDatagram`. Sign convention: `offset = remote_time -
  local_time`. Implemented as safe byte assembly with a layout test; the sign
  and live behavior require real-`chronyd` lab validation.

## 9. CLI / API / Config Notes

- CLI: `chronos-server --config /etc/chronos/server.yaml`,
  `chronos-gateway --config /etc/chronos/gateway.yaml`.
- `GET /time` JSON: `version`, `unix_sec`, `unix_nano`, `server_recv_unix_nano`,
  `server_send_unix_nano`, `status { provider, sync, stratum, last_offset_nanos
  }`; headers `Cache-Control: no-store`, `Content-Type: application/json`.
- `GET /healthz`: `{ "status": "ok" }`. `GET /status`: service/state plus
  time/backend/sample/chrony detail.
- Server config: `server.listen`, `tls.{enabled,cert_file,key_file}`,
  `api.cache_control`, `time_status.{provider,chronyc_path,allow_unknown_status}`,
  `logging.{level,format}`.
- Gateway config: `backends[] {name,url,require_tls,require_valid_cert}`,
  `sampling {interval_seconds,burst_samples,min_good_samples,max_rtt_ms,
  outlier_threshold_ms}`, `chrony {sock_path,refid}`,
  `security {allow_plain_http_backends,allow_plain_http_loopback,pinned_spki}`,
  `status.listen`, `logging`.
- Deployment config patterns: run the server container listening on a high port
  (e.g. `0.0.0.0:8080`) and publish a host port to it; for a remote plain-HTTP
  backend the gateway must set `require_tls: false` and
  `security.allow_plain_http_backends: true` (only acceptable on trusted
  networks).

## 10. Implementation Plan

Product (v1) — completed milestones:

- M1 workspace + crate skeletons; M2 `chronos-core` domain/ports/helpers; M3
  server HTTP API; M4 server native HTTPS; M5 gateway backend client; M6
  sampling + filtering; M7 chrony SOCK refclock writer + gateway status API; M8
  integration/deployment docs; M9 single combined Docker image + Compose; M10
  systemd packaging; M11 reliability hardening; M12 security hardening (incl.
  real SPKI pinning). Validation gate (`cargo fmt --check`, `cargo clippy
  --all-targets --all-features -- -D warnings`, `cargo test`) is green.

Toward production:

- Establish a chrony time source reachable by `chronos-server` so `/time`
  reports `synchronized`.
- Provision gateway hosts with `chronyd`, the `/run/chronos` runtime directory,
  and the `CHRO` SOCK refclock; deploy `chronos-gateway` against them.
- Validate the end-to-end path in a lab before relying on it in production.

## 11. Non-goals

- v1 excludes: a builtin NTP server, direct system-clock adjustment, replacing
  chrony, PTP/GPS/hardware-clock support, using arbitrary website `Date` headers
  as a primary time source, and serving internal NTP directly from
  `chronos-gateway`.
- v2 (only if chrony becomes a blocker) adds output backends
  (`builtin_ntp_server`, `direct_clock_setter`) without rewriting the v1 core.

## 12. Open Questions

- **Image version string scheme.** The develop plan specifies version `v1`
  (`ghcr.io/maple52046/chronos:v1-${ts}`), but an alternative semantic-version
  form (e.g. `1.0.0-${ts}`) has also been used in practice. Pick one canonical
  version scheme before publishing images.
- **Time source dependency.** End-to-end sync requires chrony at the server's
  upstream and `chronyd` + SOCK refclock on each gateway host; without it the
  gateway rejects all samples. Decide the standard way to guarantee this in each
  target environment.
- chrony offset sign and exact `timeval` field widths still need confirmation
  against a real `chronyd` in a lab.

## 13. Future Work

- v2 output backends: builtin NTP server and direct clock setter, behind the
  existing `OutputBackend` port.
- Optional mTLS and response signing (design-only in v1).
- Full lab validation topology (server+Nginx, gateway+chronyd, internal
  timesyncd and chrony clients) with firewall rules enforcing HTTPS-only egress
  and blocked public UDP/123.
