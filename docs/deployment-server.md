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
  provider: "chrony"
  chronyc_path: "/usr/bin/chronyc"
  allow_unknown_status: false
logging:
  level: "info"
  format: "json"
```

```bash
chronos-server --config /etc/chronos/server.yaml
curl -fsS http://127.0.0.1:8080/time
```

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
[`examples/compose/docker-compose.server.yml`](../examples/compose/docker-compose.server.yml)
and [`deployment-gateway.md`](deployment-gateway.md) for build instructions.

## Logging

Structured fields (JSON or text per `logging.format`): timestamp, level, method,
path, status, and the time-status summary. `RUST_LOG` overrides `logging.level`
at runtime.
