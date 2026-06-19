# Security

Chronos is designed for environments that permit only HTTPS egress. Its security
posture centers on the gateway's backend transport policy, strict sample
rejection, and TLS validation.

## Backend transport policy

Configured under `security` in the gateway config and enforced in
`chronos-core` (`SecurityPolicy`) **before any request is issued**:

```yaml
security:
  allow_plain_http_backends: false   # remote plain-HTTP backends
  allow_plain_http_loopback: true    # plain-HTTP to 127.0.0.1 / ::1 / localhost
  pinned_spki: []                    # optional SPKI pin set (see below)
```

Rules:

| Backend transport | Decision |
| --- | --- |
| `https://…` | Always allowed; certificate and hostname are validated. |
| `http://` loopback (`127.0.0.1`, `::1`, `localhost`) | Allowed only when `allow_plain_http_loopback` is `true`. |
| `http://` remote host | Rejected by default; allowed only when `allow_plain_http_backends` is `true`. |

In addition, a backend with `require_tls: true` is rejected at construction if
its URL is not `https`. A rejected backend fails fast at startup rather than
silently sampling over an insecure transport.

## TLS validation

HTTPS backends are validated by `rustls` (certificate chain + hostname). Setting
`require_valid_cert: false` is a **lab-only** escape hatch that disables
verification (accepts any presented certificate); never use it in production.

## Optional SPKI pinning

`security.pinned_spki` accepts base64-encoded SHA-256 hashes of the backend's
Subject Public Key Info (the same value used by HPKP / `openssl … | openssl dgst
-sha256 -binary | base64`). When the list is non-empty, the gateway additionally
requires the server's leaf-certificate SPKI hash to be in the set, on top of
normal chain validation. Pin entries are validated for format at startup; a
malformed pin aborts startup. An empty list disables pinning.

Compute a pin from a server certificate:

```bash
openssl x509 -in server.crt -pubkey -noout \
  | openssl pkey -pubin -outform der \
  | openssl dgst -sha256 -binary \
  | openssl base64
```

## Bad-sample rejection

A sample is rejected (and never written to chrony) if:

1. the HTTP request failed;
2. TLS validation failed;
3. the response JSON was invalid;
4. the backend reported `sync != synchronized`;
5. the round-trip time exceeded `max_rtt_ms`;
6. the offset was an outlier beyond `outlier_threshold_ms`;
7. fewer than `min_good_samples` survived the round.

A round with no usable sample leaves the previous good sample in place and moves
the gateway to `degraded` / `unsynchronized`. During an outage the gateway feeds
no time at all rather than bad time.

## Nginx hardening (reverse-proxy mode)

The shipped server block
([`packaging/nginx/chronos-server.conf`](../packaging/nginx/chronos-server.conf))
should:

1. terminate TLS;
2. forward only `/time`, `/healthz`, `/status`;
3. disable caching and set `Cache-Control: no-store` for `/time`;
4. optionally apply a source-IP allowlist (`allow`/`deny`);
5. optionally require client certificates (mTLS) for `/time`.

## v1 non-goals / deferred

- mTLS and response signing are design-level notes in v1, not implemented.
- The gateway never calls `clock_settime`/`adjtimex`/`settimeofday`; clock
  discipline is entirely `chronyd`'s responsibility.
