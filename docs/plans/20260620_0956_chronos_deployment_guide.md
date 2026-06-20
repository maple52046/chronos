# Chronos Deployment Guide — Consolidated Plan

> Host names are abstracted to roles. Concrete site identifiers from the source
> runbook (real hostnames, IPs, SSH aliases, user accounts) are intentionally
> replaced with role terms and example names, since the durable value is *how*
> to deploy, not *where*. Example placeholders:
>
> - **public server** — host with a synchronized clock and public HTTPS ingress
>   (example domain `time.example.com`).
> - **gateway host** — host in a network that blocks outbound NTP/UDP 123; runs
>   the gateway plus `chronyd`.
> - **internal clients** — other NTP-blocked hosts that consume time from the
>   gateway (or directly over HTTPS for verification).
> - **DC subnet** — the internal network, example `10.0.0.0/24`.

## 1. Purpose

Capture a verified, repeatable procedure for deploying Chronos end to end: a
`chronos-server` exposed over HTTPS behind a shared Nginx, and a
`chronos-gateway` on an NTP-blocked host that disciplines a local `chronyd`
through a SOCK refclock. This plan generalizes a real, end-to-end-verified
deployment into a reusable runbook.

## 2. Source Scope

Consolidated from one manuscript under `docs/plans/manuscripts/`: a deployment
runbook (2026-06-20) for a server behind Nginx (`/chronos` prefix) plus a gateway
feeding `chronyd`, marked DONE with end-to-end time sync verified. `README.md`
(the consolidation spec) is excluded.

## 3. Consolidated Background

The target environment has hosts whose outbound NTP/UDP 123 is blocked (they show
Reach 0 to all public NTP), so they cannot synchronize the usual way. Chronos
carries time over HTTPS instead: a public server that *does* have correct time
and public HTTPS ingress answers the Time API; a gateway on a restricted host
samples it and feeds the local `chronyd`, which then disciplines that host's
clock and can serve NTP to the DC subnet.

```text
public server (synchronized clock, chrony)
  HTTP loopback  ->  Nginx (TLS, shared block, location /chronos/)
                          ^ HTTPS  https://time.example.com/chronos/time
                          |
gateway host (NTP-blocked)
  chronos-gateway --SOCK refclock--> chronyd --> local clock (+ NTP to DC subnet)
internal clients (NTP-blocked) ---- HTTPS or NTP ----> time source
```

A key realization from the rollout: the original idea of hosting the server on a
restricted (NTP-blocked) host fails, because such a host cannot itself be a time
source. The server must run where the clock is already correct.

## 4. Confirmed Decisions

- Run `chronos-server` on a host with a correct clock and public HTTPS ingress;
  do not host it on an NTP-blocked host.
- Terminate TLS at a shared Nginx and mount the API under a path prefix
  (`api.base_path: /chronos`) so it coexists with other services on one domain.
- The server's time-status provider is `chrony` (`time_status.provider: chrony`),
  reading the host `chronyd` via `chronyc tracking`.
- The gateway backend points at the prefixed HTTPS URL
  (`base_url: https://time.example.com/chronos`) with `require_tls: true` and
  `require_valid_cert: true` (public, valid certificate).
- The gateway writes a chrony SOCK refclock with `refid CHRO`; `chronyd`
  disciplines the gateway clock.
- Image tag form in use is the semantic-version build tag
  `ghcr.io/maple52046/chronos:<version>-<ts>` (e.g. `1.0.0-<ts>`).

## 5. Architecture and Design Principles

- Time flows inward over HTTPS only: public server -> Nginx (TLS) -> gateway ->
  `chronyd` -> local clock / internal NTP. No outbound UDP/123 is required on the
  restricted side.
- Nginx mounts the API with `location /chronos/ { proxy_pass http://<upstream>; }`.
  Using `proxy_pass` **without** a URI path preserves the `/chronos/...` prefix so
  it matches the server's `api.base_path`.
- The gateway never sets the clock directly; `chronyd` owns discipline and the
  gateway only submits refclock samples.

## 6. Functional Scope

- Build/prepare a server image that includes `chronyc` (the published image does
  not ship it — see Constraints), deploy it on the public server, and expose it
  under `/chronos` via Nginx.
- Deploy the gateway on each restricted host, wire its chrony SOCK refclock, and
  verify the local clock synchronizes.
- Optionally point additional internal clients at the gateway's NTP service, or
  at the HTTPS endpoint for verification.

## 7. Constraints and Rules

- **`chronyc` is not in the published image.** To use the chrony status provider
  on the server, build a derived image that adds chrony, e.g. a
  `Dockerfile.chronyc`: `FROM <published-image>`, `USER root`,
  `apt-get install chrony`. (Alternatively, supply `chronyc` another way.)
- **chrony SOCK path must live under `/run/chrony/`.** On Ubuntu the AppArmor
  profile for `chronyd` only permits sockets under `@{run}/chrony/` (and
  `@{run}/chrony.*.sock`). A socket under `/run/chronos/...` is DENIED at `mknod`.
  Use e.g. `/run/chrony/chronos.sock`, not `/run/chronos/chronos.sock`.
- The server container needs read access to the host `chronyd` runtime: mount the
  host `/run/chrony` into the container so in-container `chronyc tracking` talks
  to the host daemon.
- `/run/chrony` is a systemd `RuntimeDirectory`: it is removed if `chronyd` is
  fully stopped, leaving server/gateway bind mounts stale. A
  `docker compose restart` re-establishes them after chrony restarts.
- Disable `systemd-timesyncd` on the public server where chrony is the source.
- Installing/configuring chrony and editing Nginx require host privileges (sudo),
  typically performed by the host operator.

## 8. Data Model and Format Notes

- Server `/chronos/time` returns the v1 JSON Time API; `status.sync` should be
  `synchronized` with a real `stratum` (e.g. 3) when the host chrony is healthy.
- Gateway `/status` reports `state`, the selected `backend`, `rtt`, sample
  `quality`, and `chrony.last_write`.
- On the client, `chronyc sources` shows the refclock as `#* CHRO` when selected,
  and `chronyc tracking` reports `Ref ID: CHRO`.

## 9. CLI / API / Config Notes

- Server `server.yaml`: `server.listen: 127.0.0.1:<loopback-port>` (loopback only,
  behind Nginx), `api.base_path: /chronos`, `time_status.provider: chrony`.
- Server compose: host networking, run as root (`user: "0:0"`), mount
  `./server.yaml` and the host `/run/chrony`.
- Nginx (inside an existing TLS server block):

```nginx
upstream chronos_upstream { server 127.0.0.1:<loopback-port>; }
location /chronos/ { proxy_pass http://chronos_upstream; }
```

- Gateway `gateway.yaml`: backend `base_url: https://time.example.com/chronos`,
  `require_tls: true`, `require_valid_cert: true`.
- Gateway chrony drop-in (`/etc/chrony/conf.d/chronos.conf`):
  `refclock SOCK /run/chrony/chronos.sock refid CHRO poll 4 filter 8` and
  `allow 10.0.0.0/24` (the DC subnet, to serve internal NTP).
- Gateway compose: host networking, run as root (`user: "0:0"`), mount
  `./gateway.yaml` and `/run/chrony`.

## 10. Implementation Plan

1. Confirm the public server's clock is synchronized (chrony, NTP egress OK) and
   that it has public HTTPS ingress with a valid certificate.
2. On the public server, build the `chronyc`-enabled image (`Dockerfile.chronyc`)
   from the published image.
3. Install/enable chrony on the public server (disable `systemd-timesyncd`).
4. Deploy `chronos-server` (compose: host net, root, mount `server.yaml` and
   `/run/chrony`); set `api.base_path: /chronos` and the chrony provider.
5. Add the `location /chronos/` block to the shared Nginx and reload; confirm
   `https://time.example.com/chronos/time` returns `sync: synchronized`.
6. On each gateway host, add the chrony SOCK refclock drop-in (socket under
   `/run/chrony/`) and the `allow <DC subnet>` line; restart chrony.
7. Deploy `chronos-gateway` (compose: host net, root, mount `gateway.yaml` and
   `/run/chrony`) pointing at the prefixed HTTPS backend.
8. Verify (see below); point internal clients at the gateway as needed.

## 11. Non-goals

- Not hosting the server on an NTP-blocked host (it cannot be a time source).
- Not running the gateway with `CAP_SYS_TIME` or letting it set the clock
  directly; `chronyd` owns discipline.
- Not documenting site-specific access details (SSH tunnels/aliases, accounts,
  concrete IPs) — those are operational, not part of the reusable procedure.

## 12. Open Questions

- **Root + host networking trade-off.** The verified deployment ran both
  containers as root with host networking so the server could read host chrony
  and the gateway could write the chronyd-owned SOCK without uid/permission
  friction. A hardened variant (dedicated uid/gid and socket permissions, or
  rootless) is desirable but unverified.
- **`chronyc` packaging.** Requiring a locally derived `Dockerfile.chronyc` is
  friction; whether the published image should include `chronyc`, or the server
  should support a non-`chronyc` status source, is unresolved.

## 13. Future Work

- Provide a first-class image (or build target) that includes `chronyc`, or an
  alternative server time-status provider that does not shell out to `chronyc`.
- Harden the runtime away from root + host networking (explicit uid/gid, socket
  ownership, least privilege).
- Automate the gateway-host chrony drop-in and socket-path setup, encoding the
  `/run/chrony/` AppArmor constraint so it is not rediscovered per deployment.
- Document the multi-client topology (internal NTP fan-out from the gateway) as a
  validated reference.
