# chronos-gateway on Kubernetes

Deploys `chronos-gateway` in front of a node-local NTP daemon. Shared Kubernetes
resources live in [`base`](base); daemon-specific gateway configuration lives in
one overlay per daemon.

```text
gateway/
  base/      # shared Namespace and Deployment
  ntpsec/    # ntp_shm ConfigMap for ntpsec
  ntpd/      # ntp_shm ConfigMap for classic ntpd
  chrony/    # chrony_sock ConfigMap plus the small socket-access patch
```

The `ntpd` and `ntpsec` overlays run `chronos-gateway` non-root and publish
samples into the **node's** SysV SHM refclock (`127.127.28.<unit>`). The
`chrony` overlay writes chronyd's SOCK refclock socket and therefore patches the
shared Deployment for root + hostPath access.

## Prerequisites

1. The node already runs the matching NTP daemon and refclock input.

   For ntpsec (`ntpsec/configmap.yaml`, default `unit: 2`):

   ```conf
   refclock shm unit 2 refid SHM
   ```

   For classic ntpd (`ntpd/configmap.yaml`, default `unit: 2`):

   ```conf
   server 127.127.28.2 mode 1 prefer
   fudge 127.127.28.2 refid SHM
   ```

   For chrony (`chrony/configmap.yaml`):

   ```conf
   refclock SOCK /run/chrony/chronos.sock refid CHRO poll 4 filter 8
   ```

   Restart the daemon after editing its config.

2. The `chronos` namespace enforces the `privileged` Pod Security profile from
   [`base/namespace.yaml`](base/namespace.yaml). The SHM overlays need
   `hostIPC`; the chrony overlay needs hostPath access to `/run/chrony`.

## Configure

Edit the server URL in the overlay ConfigMap you plan to deploy:
`data.gateway.yaml -> backends[0].base_url`.

## Apply

Choose the overlay that matches the node daemon:

```bash
kubectl apply -k examples/k8s/gateway/ntpsec
kubectl apply -k examples/k8s/gateway/ntpd
kubectl apply -k examples/k8s/gateway/chrony
```

## Verify

```bash
kubectl -n chronos rollout status deploy/chronos-gateway
kubectl -n chronos logs deploy/chronos-gateway -f   # expect "wrote sample to output backend"
```

On the node, use your daemon's usual inspection command: `ntpq -p` for
ntpd/ntpsec, or `chronyc sources -v` for chrony.

The status endpoint binds `127.0.0.1:9090` inside the pod, so health is checked
via the binary's `healthcheck` subcommand (an `exec` probe), not an HTTP probe.
To inspect status manually:

```bash
kubectl -n chronos exec deploy/chronos-gateway -- chronos-gateway healthcheck --config /etc/chronos/gateway.yaml
```

## Notes

- **Single node vs. multi-node.** This is a `Deployment` with `replicas: 1`,
  which feeds one node's daemon (the node the pod lands on). To discipline every
  node, convert it to a `DaemonSet` (drop `replicas`/`strategy`, change `kind`)
  so each node's daemon gets fed locally.
- **Prefer SHM for non-root pods.** The `ntpd` and `ntpsec` overlays use
  `ntp_shm`; the `chrony` overlay must run as root because chronyd owns the
  SOCK refclock socket.
- **systemd-timesyncd is not a target.** It has no SOCK/SHM refclock input; run
  ntpd/ntpsec (or chrony) on the node instead.
