# chrony Integration

`chronos-gateway` never adjusts the system clock. It writes time samples to
chrony's SOCK refclock; `chronyd` is the sole clock disciplinarian and the NTP
server for the internal network.

## SOCK refclock wire format

chrony's `refclock SOCK` driver reads a fixed C `struct sock_sample` from a Unix
**datagram** socket by raw memory copy, so fields use the host's native integer
sizes and byte order. The gateway and `chronyd` always run on the same host, so
`chronos-chrony` encodes the sample in **native endianness**. On the common
x86-64 / aarch64 Linux ABI (`time_t` and `suseconds_t` are 8-byte signed
integers) the struct is 40 bytes:

```text
offset  size  field                      type
  0      8    struct timeval.tv_sec      time_t      (i64)
  8      8    struct timeval.tv_usec     suseconds_t (i64)
 16      8    offset                     double      (f64)
 24      4    pulse                      int         (i32)  = 0
 28      4    leap                       int         (i32)  = 0 (normal)
 32      4    _pad                       int         (i32)  = 0
 36      4    magic                      int         (i32)  = 0x534F434B ("SOCK")
```

- `tv_sec` / `tv_usec`: the local time at which the sample is valid (the
  gateway's wall-clock receive time).
- `offset`: in seconds. Chronos uses the convention **`offset = remote_time -
  local_time`** (a positive value means the local clock is behind the backend),
  matching the gateway's offset estimator.
- `magic`: `0x534F434B`. On a little-endian host the four bytes at offset 36 read
  as `KCOS`; chrony reconstructs the native `int` and compares it to
  `0x534F434B`, so this is correct.

> The exact `timeval` field widths and the offset sign convention can only be
> fully validated against a real `chronyd`. This is a manual/lab acceptance step
> (milestones M7/M8); the encoding is covered by a byte-layout unit test in
> `chronos-chrony`.

## Gateway `chrony.conf`

See [`examples/config/chrony.gateway.conf`](../examples/config/chrony.gateway.conf):

```conf
refclock SOCK /run/chronos/chrony.sock refid CHRO poll 4 filter 8
allow 192.168.100.0/24
# local stratum 10   # optional isolated-mode fallback, disabled by default
```

The `SOCK` socket path and `refid` must match the gateway's `chrony.sock_path`
and `chrony.refid` settings.

## Shared runtime directory

`chronyd` creates `/run/chronos/chrony.sock`; `chronos-gateway` writes to it. The
socket permissions must let the gateway write. Provision the directory with the
shipped tmpfiles fragment
([`packaging/tmpfiles.d/chronos.conf`](../packaging/tmpfiles.d/chronos.conf)):

```conf
d /run/chronos 0755 chronos chronos -
```

If `chronyd` runs as a different user, adjust ownership accordingly:

```bash
ps -eo user,group,comm | grep chronyd
```

## Validating against a real chronyd

1. Start `chronyd` with the SOCK refclock configured, then start
   `chronos-gateway`.
2. Confirm the refclock is seen and accepting samples:

   ```bash
   chronyc sources      # expect a line with refid CHRO (#? / #* / #+)
   chronyc sourcestats
   chronyc tracking
   ```

3. Confirm the gateway never sets the clock itself — only `chronyd` should:

   ```bash
   # chronos-gateway must not call clock_settime/adjtimex.
   sudo strace -f -e trace=clock_settime,adjtimex,settimeofday \
       /usr/local/bin/chronos-gateway --config /etc/chronos/gateway.yaml
   ```

4. If `chronyc sources` shows `CHRO` reachable and `tracking` converges, the
   refclock integration is working. If the offset drives the clock the wrong
   way, the sign convention above is inverted for your build/ABI — re-check
   before production use.
