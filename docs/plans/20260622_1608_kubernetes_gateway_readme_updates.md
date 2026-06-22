# Kubernetes Gateway and README Updates

## Purpose

Preserve the planning decisions behind the recent README and Kubernetes example
updates: the root README should explain the high-level Chronos system and expose
Kubernetes deployment entry points, while the gateway Kubernetes examples should
avoid duplicated common manifests.

## Source Scope

This consolidation summarizes three manuscript plans from
`docs/plans/manuscripts/`:

- `20260622-readme-architecture-diagram.md`
- `20260622-k8s-gateway-layout.md`
- `20260622-readme-kubernetes-deployment.md`

## Consolidated Background

Chronos needs approachable top-level documentation for readers who want to
understand the system quickly and deploy the gateway on Kubernetes. The README
architecture diagram is intended to be a high-level deployment/data-flow diagram,
not an internal crate, trait, or compile-time dependency diagram.

The Kubernetes gateway examples had duplicated resources across daemon variants.
Common Kubernetes resources should be defined once, and daemon-specific
configuration should live under overlays for the node-local NTP daemon.

## Confirmed Decisions

- The root README includes a repository-owned SVG architecture diagram under
  `docs/`.
- The architecture diagram stays at the deployment/data-flow level: synced
  server-side host, Chronos Time API, restricted data center, `chronos-gateway`,
  local NTP daemon, and internal hosts.
- Gateway Kubernetes examples use a shared `base/` and daemon-specific overlays.
- Supported gateway overlays are `ntpsec/`, `ntpd/`, and `chrony/`.
- Users apply exactly one daemon-specific overlay directly with
  `kubectl apply -k`.

## Architecture and Design Principles

Documentation should emphasize operational boundaries before implementation
details. The high-level diagram should show that Chronos moves time samples over
HTTP/HTTPS and hands them to the local NTP stack, allowing restricted
environments to avoid outbound NTP.

Kubernetes examples should follow a base-and-overlay structure. Shared resources
belong in `base/`; only daemon-specific configuration and daemon-specific
deployment differences belong in per-daemon overlays.

## Functional Scope

- Add or maintain the README architecture diagram near the top of `README.md`.
- Add or maintain a root README Kubernetes section after Docker.
- Link Kubernetes users to `examples/k8s/gateway`.
- Keep shared gateway Kubernetes resources in `examples/k8s/gateway/base/`.
- Keep `ntpd` and `ntpsec` differences in their own ConfigMaps.
- Keep the chrony-only Deployment changes in a small patch.

## Constraints and Rules

- Do not change Rust code for these documentation and example-structure tasks.
- Do not change the container image as part of the layout work.
- Do not introduce Kubernetes behavior changes beyond the intended file layout
  and necessary overlay composition.
- Do not make the architecture diagram a crate-level, trait-level, or
  compile-time dependency diagram.
- Do not attempt to make `chrony_sock` run non-root; socket ownership and daemon
  runtime-directory permissions require root-oriented access in the overlay.

## Data Model and Format Notes

The architecture diagram is an SVG file stored in the repository so Markdown
renderers can display it directly. It should represent deployment/data-flow
concepts rather than internal data structures.

Kubernetes manifests remain YAML and are organized for Kustomize overlays.
Gateway configuration is embedded in daemon-specific `ConfigMap` resources under
`data.gateway.yaml`.

## CLI / API / Config Notes

The root README Kubernetes section should show these apply entry points:

```bash
kubectl apply -k examples/k8s/gateway/ntpsec
kubectl apply -k examples/k8s/gateway/ntpd
kubectl apply -k examples/k8s/gateway/chrony
```

Before applying, users should edit the selected overlay's `configmap.yaml` and
set `data.gateway.yaml -> backends[0].base_url` to the Chronos server URL.

## Implementation Plan

1. Keep the high-level SVG diagram under `docs/` and embed it from `README.md`.
2. Keep the root README Kubernetes section concise and point detailed
   daemon-specific instructions to `examples/k8s/gateway/README.md`.
3. Maintain the gateway example layout:

   ```text
   examples/k8s/gateway/
     README.md
     base/
       kustomization.yaml
       namespace.yaml
       deployment.yaml
     chrony/
       kustomization.yaml
       configmap.yaml
       deployment-patch.yaml
     ntpd/
       kustomization.yaml
       configmap.yaml
     ntpsec/
       kustomization.yaml
       configmap.yaml
   ```

4. Ensure each overlay references `../base`.
5. Render the `ntpsec`, `ntpd`, and `chrony` overlays with `kubectl kustomize`
   after manifest changes.

## Non-goals

- No Rust code changes.
- No generated binary image assets.
- No new server-side Kubernetes example in this planning scope.
- No behavior or configuration compatibility work unrelated to the README and
  gateway Kubernetes examples.

## Open Questions

- Whether a dedicated Kubernetes server example should be added later remains
  undecided.
- Whether the gateway example should eventually provide a DaemonSet variant for
  disciplining every node remains future work.

## Future Work

- Consider adding a Kubernetes server example if deployment guidance needs to
  cover both `chronos-server` and `chronos-gateway`.
- Consider a DaemonSet overlay for environments where every node-local NTP daemon
  should be fed by a local gateway pod.
- Keep the README diagram and gateway example README in sync with any future
  supported output backends.
