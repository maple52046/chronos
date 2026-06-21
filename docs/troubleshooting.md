# Troubleshooting

## chronyc sources does not show CHRO

- Confirm `chronyd` created the socket: `ls -l /run/chrony/chronos.sock`.
- Confirm chrony config has `refclock SOCK /run/chrony/chronos.sock refid CHRO`
  (e.g. in `/etc/chrony/conf.d/chronos.conf`) and that `refid` matches the
  gateway's `chrony.refid`.
- Confirm the gateway is writing: `curl -s 127.0.0.1:9090/status` should show
  `chrony.last_write: "ok"`. If it shows an error, the socket path or
  permissions are wrong.
- Start order matters: restart `chronyd` (so the socket exists) before starting
  the gateway. The systemd unit is ordered `After=chronyd.service`.

## Gateway status shows `last_write: "error: …"`

- `error: …No such file…`: the socket does not exist. chronyd creates it only
  when the `refclock SOCK` directive is present; (re)start chronyd, then confirm
  `ls -l /run/chrony/chronos.sock`. The gateway must also start after chronyd.
- `error: …Permission denied`: chronyd creates the socket owned by **root**
  (`srwxr-xr-x`), so the gateway must run as root. In a container set
  `user: "0:0"`; with systemd use the shipped unit (runs as root). Verify the
  path is bind-mounted into the container (`/run/chrony:/run/chrony`).

## Gateway state is `degraded` or `unsynchronized`

- `degraded`: it was synchronized but recent rounds failed; the last good sample
  may still be valid. `unsynchronized`: it has never produced a good sample.
- Inspect logs for the per-round summary (`attempts`, `good`, `high_latency`,
  `outliers`, `failures`).
- Common causes: backend unreachable (egress firewall), backend reporting
  `sync: unsynchronized`, RTT consistently above `max_rtt_ms`, or fewer than
  `min_good_samples` survivors. Raise `max_rtt_ms` for high-latency links.

## First sync is slow (CHRO stays `#?`, clock not stepped)

On a host whose clock starts far off (tens of seconds), chronyd must collect
several consistent samples before it selects `CHRO` and `makestep` steps the
clock. If it stays unselected (`#?`) for many minutes:

- Check the cadence. Size the chrony refclock `poll` so chronyd gets at least
  two samples per poll: `2^poll >= 2 * interval_seconds`. If they are too close
  (a 30 s interval with `poll 5` = 32 s, or interval > 2^poll) jitter leaves many
  polls empty — `chronyc sources` shows a low/sparse `Reach` and `sourcestats`
  shows `NP` stuck low with a huge `Std Dev`. The simplest fix is to regenerate
  the drop-in from your `gateway.yaml` (it computes the right `poll` for you):
  `sudo packaging/setup-chrony-refclock.sh --config /etc/chronos/gateway.yaml --install`.
  Otherwise set it by hand, e.g. `interval_seconds: 8` with `poll 4` (16 s), or
  `interval_seconds: 30` with `poll 6` (64 s), and restart both the gateway and
  chronyd.
- Confirm `makestep` is present in the chrony config; without it chronyd only
  slews and a large offset takes a very long time to correct.
- Watch progress: `chronyc sourcestats` (`NP` should grow, `Std Dev` shrink)
  until `chronyc sources` shows `#* CHRO` and `chronyc tracking` reports
  `Leap status: Normal`.

## Backend rejected at startup

- `rejected by security policy`: a remote `http://` backend with
  `allow_plain_http_backends: false`. Use HTTPS or, for loopback only, rely on
  `allow_plain_http_loopback`.
- `require_tls … is not https`: the backend URL is `http://` but `require_tls:
  true`.

## Sampling fails with HTTP 404

- The gateway `base_url` and the server `api.base_path` disagree. The gateway
  requests `<base_url>/time`, so when the server is mounted under `/chronos` the
  `base_url` must end in `/chronos`. Confirm with
  `curl -fsS <base_url>/time`.

## TLS errors when sampling

- The sample quality logs as `TlsError`. Verify the certificate chain and that
  the hostname matches the URL. For a lab self-signed server, set
  `require_valid_cert: false` (never in production), or add the backend's SPKI to
  `security.pinned_spki`.

## chronos-server `/time` shows `sync: "unknown"`

- The status provider could not be read. For the `system` provider, the kernel
  reports the clock as unsynchronized (no disciplining daemon has converged, or
  the daemon does not maintain the kernel NTP flag). For the `chrony` provider,
  verify `time_status.chrony_address` reaches a running `chronyd` command port
  (default `127.0.0.1:323`; use host networking in a container). The server still
  serves time; only the status is unknown.

## Clock moves the wrong way

- The chrony offset sign convention may be inverted for your ABI/build. Stop the
  gateway and re-validate against `chronyd` per
  [`chrony-integration.md`](chrony-integration.md) before production use.

## Useful commands

```bash
chronyc sources -v
chronyc tracking
chronyc sourcestats
journalctl -u chronos-gateway -f
journalctl -u chronos-server -f
curl -fsS 127.0.0.1:9090/status | jq
```
