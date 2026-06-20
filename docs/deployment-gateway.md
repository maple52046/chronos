# Deploying chronos-gateway (with chronyd)

`chronos-gateway` runs on a host inside the restricted network alongside
`chronyd`. It samples a `chronos-server` over HTTP/HTTPS and writes good samples
to chrony's SOCK refclock. `chronyd` disciplines the host clock and serves NTP to
the rest of the data center.

## Ubuntu / Debian host setup

```bash
sudo apt update
sudo apt install chrony

# chrony, not timesyncd, owns the clock on this host.
sudo systemctl disable --now systemd-timesyncd || true
sudo systemctl enable --now chrony

sudo useradd --system --no-create-home --shell /usr/sbin/nologin chronos || true
sudo install -d -o chronos -g chronos -m 0755 /run/chronos
```

Install the chrony config
([`examples/config/chrony.gateway.conf`](../examples/config/chrony.gateway.conf)):

```conf
refclock SOCK /run/chronos/chrony.sock refid CHRO poll 4 filter 8
allow 192.168.100.0/24
```

```bash
sudo systemctl restart chrony
```

Install the gateway:

```bash
sudo install -m 0755 target/release/chronos-gateway /usr/local/bin/chronos-gateway
sudo install -D -m 0644 examples/config/gateway.yaml /etc/chronos/gateway.yaml
sudo install -m 0644 packaging/systemd/chronos-gateway.service \
    /etc/systemd/system/chronos-gateway.service
sudo install -m 0644 packaging/tmpfiles.d/chronos.conf \
    /usr/lib/tmpfiles.d/chronos.conf
sudo systemd-tmpfiles --create
sudo systemctl daemon-reload
sudo systemctl enable --now chronos-gateway
```

Verify:

```bash
curl -fsS http://127.0.0.1:9090/status
chronyc sources         # expect refid CHRO
chronyc tracking
```

The gateway unit is ordered `After=chronyd.service` so the socket exists at
start.

## Gateway configuration

See [`examples/config/gateway.yaml`](../examples/config/gateway.yaml). Key
sections: `backends` (ordered; earlier entries preferred), `sampling`
(`interval_seconds`, `burst_samples`, `min_good_samples`, `max_rtt_ms`,
`outlier_threshold_ms`), `chrony` (`sock_path`, `refid`), `security`, and
`status` (`listen`). See [`security.md`](security.md) for the transport policy.

Each backend's `base_url` is the Chronos server's base URL without the endpoint;
the gateway appends `/time`. Include the server's `api.base_path` when set, e.g.
`base_url: "https://time.example.com/chronos"`.

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

The design assumes a data center that allows only HTTPS egress. Enforce:

```text
gateway  -> chronos-server : allow HTTPS 443 (or the server port)
gateway  -> public NTP     : block UDP/123
internal -> gateway        : allow UDP/123
internal -> public NTP     : block UDP/123
```

This keeps all external time flowing over the audited HTTPS path while the
gateway provides standard NTP internally.

## Container note

If you run the gateway as a container, use `network_mode: host` and bind-mount
`/run/chronos` so it can reach the host chrony socket
([`examples/compose/docker-compose.gateway.yml`](../examples/compose/docker-compose.gateway.yml)).
The container needs no `CAP_SYS_TIME` and never touches the system clock. If
socket permissions are awkward, prefer the host systemd service.
