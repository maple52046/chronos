# Troubleshooting

## chronyc sources does not show CHRO

- Confirm `chronyd` created the socket: `ls -l /run/chronos/chrony.sock`.
- Confirm `chrony.conf` has `refclock SOCK /run/chronos/chrony.sock refid CHRO`
  and that `refid` matches the gateway's `chrony.refid`.
- Confirm the gateway is writing: `curl -s 127.0.0.1:9090/status` should show
  `chrony.last_write: "ok"`. If it shows an error, the socket path or
  permissions are wrong.
- Start order matters: the gateway must start after `chronyd` (the unit is
  ordered `After=chronyd.service`).

## Gateway status shows `last_write: "error: …"`

- The socket is missing or unwritable. Check ownership of `/run/chronos` and the
  user `chronyd`/`chronos-gateway` run as (`ps -eo user,group,comm | grep
  chronyd`).
- Provision the directory: `sudo systemd-tmpfiles --create` with the shipped
  `tmpfiles.d/chronos.conf`.

## Gateway state is `degraded` or `unsynchronized`

- `degraded`: it was synchronized but recent rounds failed; the last good sample
  may still be valid. `unsynchronized`: it has never produced a good sample.
- Inspect logs for the per-round summary (`attempts`, `good`, `high_latency`,
  `outliers`, `failures`).
- Common causes: backend unreachable (egress firewall), backend reporting
  `sync: unsynchronized`, RTT consistently above `max_rtt_ms`, or fewer than
  `min_good_samples` survivors. Raise `max_rtt_ms` for high-latency links.

## Backend rejected at startup

- `rejected by security policy`: a remote `http://` backend with
  `allow_plain_http_backends: false`. Use HTTPS or, for loopback only, rely on
  `allow_plain_http_loopback`.
- `require_tls … is not https`: the backend URL is `http://` but `require_tls:
  true`.

## TLS errors when sampling

- The sample quality logs as `TlsError`. Verify the certificate chain and that
  the hostname matches the URL. For a lab self-signed server, set
  `require_valid_cert: false` (never in production), or add the backend's SPKI to
  `security.pinned_spki`.

## chronos-server `/time` shows `sync: "unknown"`

- The status provider could not be read. For the chrony provider, verify
  `time_status.chronyc_path` points to a working `chronyc` and that the server
  user may run it. The server still serves time; only the status is unknown.

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
