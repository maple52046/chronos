# Deploying chronos-server

`chronos-server` provides the HTTP Time API. It does not serve NTP and never
adjusts a client clock. It supports three transport modes.

## 1. Native HTTP

For local development, labs, and trusted private networks.

```yaml
# /etc/chronos/server.yaml
server:
  listen: "127.0.0.1:8080"
tls:
  enabled: false
api:
  cache_control: "no-store"
  base_path: ""        # e.g. "/chronos" to mount under a shared reverse proxy
time_status:
  provider: "system"            # see "Time-status providers" below
  chrony_address: "127.0.0.1:323"
  allow_unknown_status: false
logging:
  level: "info"
  format: "json"
```

```bash
chronos-server --config /etc/chronos/server.yaml
curl -fsS http://127.0.0.1:8080/time
```

## Time-status providers

`chronos-server` reports whether its host clock is synchronized so the gateway
only trusts good time. The provider is selected by `time_status.provider`:

- `system` (default): reads the kernel NTP synchronization state via
  `adjtimex(2)`. It is daemon-agnostic - any implementation that disciplines the
  kernel clock (chrony, ntpd, ntpsec, systemd-timesyncd) is reflected, with no
  external binary. It reports only the sync state (no stratum/offset detail).
  Note: a daemon that does not maintain the kernel sync flag (e.g. OpenNTPD) is
  not detected by this provider.
- `chrony`: queries `chronyd` directly over its command protocol at
  `time_status.chrony_address` (default `127.0.0.1:323`); no `chronyc` binary is
  required. Use it when you want chrony's stratum/offset detail. In a container,
  use host networking so the query reaches the host `chronyd`.
- any other value: inert; the server serves time but reports `unknown`.

See [`examples/config/server.http.yaml`](../examples/config/server.http.yaml).

## 2. Native HTTPS

Terminate TLS in the server itself (single-binary production, no reverse proxy).

```yaml
server:
  listen: "0.0.0.0:8443"
tls:
  enabled: true
  cert_file: "/etc/chronos/server.crt"
  key_file: "/etc/chronos/server.key"
```

TLS is provided by `axum-server` + `rustls`. `tls.enabled: true` requires both
`cert_file` and `key_file` (PEM); configuration validation rejects a half-set
TLS block at startup. See
[`examples/config/server.https.yaml`](../examples/config/server.https.yaml).

## 3. HTTP behind a reverse proxy (recommended for production)

Run the server on loopback and terminate TLS in Nginx/Caddy/HAProxy.

```text
chronos-gateway --HTTPS--> Nginx (:443, terminates TLS) --HTTP--> chronos-server (127.0.0.1:8080)
```

Use the native-HTTP config above and the shipped Nginx server block
([`packaging/nginx/chronos-server.conf`](../packaging/nginx/chronos-server.conf)),
which forwards only `/time`, `/healthz`, and `/status`, disables caching, and
sets `Cache-Control: no-store`.

To share one Nginx server block with other services, set `api.base_path` (e.g.
`/chronos`) and use the prefixed `location /chronos/` variant in that file; the
prefix is preserved upstream so the server must be configured with the matching
`api.base_path`. Point the gateway's `base_url` at the same prefix
(`https://time.example.com/chronos`).

## systemd

Install the unit
([`packaging/systemd/chronos-server.service`](../packaging/systemd/chronos-server.service)):

```bash
sudo useradd --system --no-create-home --shell /usr/sbin/nologin chronos || true
sudo install -m 0755 target/release/chronos-server /usr/local/bin/chronos-server
sudo install -D -m 0644 examples/config/server.http.yaml /etc/chronos/server.yaml
sudo install -m 0644 packaging/systemd/chronos-server.service \
    /etc/systemd/system/chronos-server.service
sudo systemctl daemon-reload
sudo systemctl enable --now chronos-server
```

The unit runs as the unprivileged `chronos` user with `NoNewPrivileges`,
`PrivateTmp`, `ProtectSystem=strict`, and `ProtectHome`.

## Docker

The combined image runs the server via `command`. See
[`examples/compose/docker-compose.server.yml`](../examples/compose/docker-compose.server.yml).
The image is distroless; the container `HEALTHCHECK` uses the binary's
`healthcheck` subcommand (no curl). To build the image yourself, follow the
instructions in the repository [`Dockerfile`](../Dockerfile).

With the default `system` provider, bridge networking with a published port is
enough. With `time_status.provider: "chrony"`, the container must use
`network_mode: host` so it can reach the host `chronyd` at `127.0.0.1:323` (the
example compose documents both).

## Logging

Structured fields (JSON or text per `logging.format`): timestamp, level, method,
path, status, and the time-status summary. `RUST_LOG` overrides `logging.level`
at runtime.
