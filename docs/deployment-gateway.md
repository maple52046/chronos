# Deploying chronos-gateway (with chronyd)

`chronos-gateway` runs on a host alongside `chronyd`. It samples a
`chronos-server` over HTTP/HTTPS and writes good samples to chrony's SOCK
refclock. `chronyd` is the sole clock disciplinarian: it steps/slews the host
clock and can serve NTP to the rest of the network. The gateway never touches
the system clock and needs no `CAP_SYS_TIME`.

This is the right design when a host cannot reach public NTP (UDP/123 egress is
firewalled) but can reach the Chronos server over HTTPS.

## 1. Configure chronyd (SOCK refclock)

Install chrony if needed, then add the Chronos SOCK refclock. The shipped config
([`examples/config/chrony.gateway.conf`](../examples/config/chrony.gateway.conf))
is a drop-in that does **not** replace an existing `chrony.conf`:

```bash
sudo apt update && sudo apt install -y chrony
sudo install -D -m 0644 examples/config/chrony.gateway.conf \
    /etc/chrony/conf.d/chronos.conf
sudo systemctl restart chrony
```

Or generate the drop-in automatically from your `gateway.yaml` — the helper
derives the refclock `poll` from `sampling.interval_seconds` (so you never have
to compute it), checks the socket path and `makestep`, then installs and
restarts chrony:

```bash
# Dry run (prints the recommended drop-in):
packaging/setup-chrony-refclock.sh --config examples/config/gateway.yaml
# Install it (add --allow <subnet> to also serve NTP to that network):
sudo packaging/setup-chrony-refclock.sh --config /etc/chronos/gateway.yaml --install
```

The essential directive is the refclock:

```conf
refclock SOCK /run/chrony/chronos.sock refid CHRO poll 4 filter 8
```

A large initial offset must be **stepped**, which needs `makestep` in the host's
chrony config. The Debian/Ubuntu default `chrony.conf` already has `makestep 1
3`; on a minimal/standalone config, add `makestep 1.0 3` there.

After the restart, chronyd creates the socket (owned by root) in its own runtime
directory:

```bash
ls -l /run/chrony/chronos.sock     # expect a srwxr-xr-x socket
```

> The socket path and `refid` must match the gateway's `chrony.sock_path` and
> `chrony.refid`. Because chronyd creates the socket owned by **root**, the
> gateway process must run as root to write to it (see below).

## 2. Run the gateway (container — recommended)

Use the published image with host networking, running as root so it can write
the chrony socket. See
[`examples/compose/docker-compose.gateway.yml`](../examples/compose/docker-compose.gateway.yml)
and [`examples/config/gateway.yaml`](../examples/config/gateway.yaml).

```bash
mkdir -p ~/chronos-gateway && cd ~/chronos-gateway
cp /path/to/examples/compose/docker-compose.gateway.yml docker-compose.yml
cp /path/to/examples/config/gateway.yaml gateway.yaml
# Edit gateway.yaml: set backends[].base_url to your Chronos server.
docker compose up -d
```

The image is distroless (no shell, no curl); the container `HEALTHCHECK` uses the
binary's `healthcheck` subcommand, which probes the local `status.listen`
`/healthz`.

## 3. Verify (and first-sync behavior)

```bash
docker compose logs -f chronos-gateway      # expect "wrote sample to chrony", state Synchronized
curl -fsS http://127.0.0.1:9090/status      # chrony.last_write should be "ok"
chronyc sources                             # expect a refid CHRO line
chronyc tracking                            # Reference ID becomes CHRO, Leap status Normal
```

`chronyc sources` shows the refclock state: `#?` (seen, not yet usable), `#+`
(candidate), `#*` (selected). On a host whose clock starts far off (tens of
seconds), chronyd needs several consistent samples before it selects `CHRO` and
`makestep` steps the clock. With the aligned cadence above this takes ~1-2
minutes. If it stays `#?` for longer, see
[`troubleshooting.md`](troubleshooting.md) ("First sync is slow").

## Gateway configuration

See [`examples/config/gateway.yaml`](../examples/config/gateway.yaml). Key
sections: `backends` (ordered; earlier entries preferred), `sampling`
(`interval_seconds`, `burst_samples`, `min_good_samples`, `max_rtt_ms`,
`outlier_threshold_ms`), `chrony` (`sock_path`, `refid`), `security`, and
`status` (`listen`). See [`security.md`](security.md) for the transport policy.

- Each backend's `base_url` is the Chronos server's base URL **without** the
  endpoint; the gateway appends `/time`. Include the server's `api.base_path`
  when set, e.g. `base_url: "https://time.example.com/chronos"`.
- Size `sampling.interval_seconds` against the refclock `poll` in
  `chrony.gateway.conf` so chronyd gets at least two samples per poll:
  `2^poll >= 2 * interval_seconds` (e.g. interval 8 → `poll 4` = 16 s; interval
  30 → `poll 6` = 64 s). If they are too close (a 30 s interval with `poll 5` =
  32 s) jitter leaves many polls empty, so chronyd sees sparse samples and is
  slow to select the source and step a large initial offset.

## Alternative: systemd service (no container)

```bash
sudo install -m 0755 target/release/chronos-gateway /usr/local/bin/chronos-gateway
sudo install -D -m 0644 examples/config/gateway.yaml /etc/chronos/gateway.yaml
sudo install -m 0644 packaging/systemd/chronos-gateway.service \
    /etc/systemd/system/chronos-gateway.service
sudo systemctl daemon-reload
sudo systemctl enable --now chronos-gateway
```

The unit runs as **root** (`ReadWritePaths=/run/chrony`, `NoNewPrivileges`,
`ProtectSystem=strict`, `ProtectHome`) because it must write chrony's
root-owned SOCK socket, and is ordered `After=chronyd.service` so the socket
exists at start.

## Internal client setup

Point internal servers at the gateway's NTP service (the gateway host running
`chronyd`, e.g. `192.168.100.10`).

### systemd-timesyncd client

```ini
# /etc/systemd/timesyncd.conf.d/chronos-gateway.conf
[Time]
NTP=192.168.100.10
FallbackNTP=
```

```bash
sudo systemctl restart systemd-timesyncd
timedatectl timesync-status
```

### chrony client

```conf
# /etc/chrony/chrony.conf
server 192.168.100.10 iburst
```

```bash
sudo systemctl restart chrony
chronyc sources
chronyc tracking
```

## Firewall / network policy

The design assumes a network that allows only HTTPS egress. Enforce:

```text
gateway  -> chronos-server : allow HTTPS 443 (or the server port)
gateway  -> public NTP     : block UDP/123
internal -> gateway        : allow UDP/123
internal -> public NTP     : block UDP/123
```

This keeps all external time flowing over the audited HTTPS path while the
gateway provides standard NTP internally.
