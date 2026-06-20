# Chronos HTTP Time API (v1)

`chronos-server` exposes three endpoints. All responses are
`Content-Type: application/json` and carry `Cache-Control: no-store` (configurable
via `api.cache_control`) so intermediaries never serve stale time.

The paths below are relative to the configured `api.base_path`. With the default
(empty) prefix they are served at the root (`/time`); setting
`api.base_path: "/chronos"` serves them under that prefix
(`/chronos/time`, `/chronos/healthz`, `/chronos/status`), which lets the server
share one reverse proxy with other services.

## `GET /time`

```json
{
  "version": 1,
  "unix_sec": 1781844000,
  "unix_nano": 1781844000123456789,
  "server_recv_unix_nano": 1781844000123000000,
  "server_send_unix_nano": 1781844000123456789,
  "status": {
    "provider": "chrony",
    "sync": "synchronized",
    "stratum": 3,
    "last_offset_nanos": 12000
  }
}
```

| Field | Meaning |
| --- | --- |
| `version` | API response version (always `1` in v1). |
| `unix_sec` | `server_send_unix_nano` converted to Unix seconds. |
| `unix_nano` | Same value as `server_send_unix_nano`. |
| `server_recv_unix_nano` | Time the server received the request. |
| `server_send_unix_nano` | Time immediately before the server wrote the response. |
| `status.provider` | `system_clock` \| `chrony` \| `unknown`. |
| `status.sync` | `synchronized` \| `unsynchronized` \| `unknown`. |
| `status.stratum` | Optional chrony stratum (`null` when unavailable). |
| `status.last_offset_nanos` | Optional last clock offset from chrony tracking (`null` when unavailable). |

`server_recv_unix_nano` is captured at handler entry and `server_send_unix_nano`
immediately before serialization, so a client can bound server-side processing
time. If the status provider cannot be read, the server still serves time and
reports `sync: "unknown"`.

## `GET /healthz`

```json
{ "status": "ok" }
```

Pure process liveness, intended for Docker/Compose healthchecks and load
balancers. It does not depend on synchronization state.

## `GET /status`

```json
{
  "service": "chronos-server",
  "state": "running",
  "time_status": {
    "provider": "chrony",
    "sync": "synchronized",
    "stratum": 3,
    "last_offset_nanos": 12000
  }
}
```

`state` is `running` when the sync state is known, or when it is unknown and
`time_status.allow_unknown_status` is `true`; otherwise it is `degraded`.

## Gateway sampling algorithm

For each sample the gateway records a monotonic `t0` before the request and `t3`
after the response, then:

```text
rtt    = t3 - t0
remote = response.server_send_unix_nano
offset = remote + rtt / 2 - local_wall_clock_at_receive
```

`+ rtt / 2` approximates the return-path delay so `remote + rtt/2` is the
server's time at the moment the gateway received the response. A positive offset
means the local clock is behind the backend.

Per round the gateway:

1. collects `burst_samples` samples;
2. rejects failed requests, TLS errors, invalid JSON, and unsynchronized backends;
3. rejects samples with `rtt > max_rtt_ms`;
4. computes the median offset and rejects samples deviating by more than
   `outlier_threshold_ms`;
5. requires at least `min_good_samples` survivors;
6. selects the median-offset survivor and writes it to chrony.

A round that yields no usable sample never writes to chrony; the gateway state
transitions to `degraded` (if previously synchronized) or `unsynchronized`.
