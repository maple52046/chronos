# ntpd/ntpsec SHM Integration

`chronos-gateway` never adjusts the system clock. With the `ntp_shm` output
backend it publishes time samples into the SysV shared-memory segment that
ntpd/ntpsec's `SHM(unit)` refclock (`127.127.28.<unit>`) reads. The local NTP
daemon is the sole clock disciplinarian and the NTP server for the internal
network.

Unlike `chrony_sock`, this backend needs **no root**: the segment can be created
world-writable, so a non-root gateway can write a segment the (root) daemon
reads.

## SHM segment

The driver exchanges time through a SysV shared-memory segment keyed by
`0x4E545030 + unit` ("NTP0" + unit). The gateway and the daemon run on the same
host, so the segment uses the host's native integer sizes and byte order. On the
common x86-64 / aarch64 Linux ABI (`time_t` is an 8-byte signed integer) the C
`struct shmTime` is 96 bytes:

```text
offset  size  field                      type
  0      4    mode                       int        (1 = count handshake)
  4      4    count                      int        (volatile)
  8      8    clockTimeStampSec          time_t
 16      4    clockTimeStampUSec         int
 20      4    (padding)
 24      8    receiveTimeStampSec        time_t
 32      4    receiveTimeStampUSec       int
 36      4    leap                       int        = 0 (normal)
 40      4    precision                  int
 44      4    nsamples                   int
 48      4    valid                      int        (volatile)
 52      4    clockTimeStampNSec         unsigned
 56      4    receiveTimeStampNSec       unsigned
 60     32    dummy[8]                   int[8]
```

- `receiveTimeStamp*`: the local time at which the sample is valid (the gateway's
  wall-clock receive time).
- `clockTimeStamp*`: the reference (true) time, computed as `receive +
  estimated_offset`, so the daemon observes Chronos's `remote - local` offset.
- The writer sets `mode = 1` and brackets each update by incrementing `count`
  before and after the field writes and toggling `valid` (0 → 1), so a
  concurrently reading daemon never consumes a torn sample.

The byte layout is covered by an `offset_of!`/`size_of` unit test in
`chronos-ntp`.

## Unit numbers and permissions

- `unit` maps to the daemon refclock address `127.127.28.<unit>` and must match
  the daemon config.
- ntpd convention: units `0`–`1` are private (mode `0600`, root-only); units
  `>= 2` may be shared (mode `0666`). Chronos defaults to `unit: 2` and
  `perm: "0666"` so the gateway can run non-root.
- `perm` is the SysV permission applied when the gateway **creates** the segment.
  If the daemon created it first, the existing permissions apply. A
  world-writable segment is local-only and gated by host IPC permissions; tighten
  to a shared group if your threat model requires it (run the gateway and daemon
  in the same group and use `0660`).

## Daemon configuration

```conf
# ntpsec (/etc/ntpsec/ntp.conf)
refclock shm unit 2 refid SHM
```

```conf
# classic ntpd (/etc/ntp.conf)
server 127.127.28.2 mode 1 prefer
fudge 127.127.28.2 refid SHM
```

Restart the daemon after editing its config.

## Containers

The SHM segment lives in the host's SysV IPC namespace, so a containerized
gateway must share it with `ipc: host` (see
[`examples/compose/docker-compose.gateway.yml`](../examples/compose/docker-compose.gateway.yml)).
The container runs as a non-root user and needs no `CAP_SYS_TIME`.

## Validating against a real daemon

1. Start ntpd/ntpsec with the SHM refclock configured, then start
   `chronos-gateway` with `output.type: ntp_shm`.
2. Confirm the refclock is seen and becomes reachable:

   ```bash
   ntpq -p           # expect a SHM(2) / 127.127.28.2 line; reach count rises
   ipcs -m           # expect a shared-memory segment with key 0x4e545032
   ```

3. Confirm the gateway never sets the clock itself — only the daemon should:

   ```bash
   strace -f -e trace=clock_settime,adjtimex,settimeofday \
       /usr/local/bin/chronos-gateway --config /etc/chronos/gateway.yaml
   ```

4. If `ntpq -p` shows the SHM refclock reachable and the daemon's offset
   converges, the integration is working. If the offset drives the clock the
   wrong way, the `remote - local` sign convention is inverted for your
   build/ABI — re-check before production use.
