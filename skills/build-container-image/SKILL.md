---
name: build-container-image
description: >-
  Build the Chronos container image from the repository Dockerfile and tag it as
  `${repo}:${version}-${ts}`, optionally pushing afterwards. Use when the user
  runs /build-container-image or asks to build/push the Chronos container image.
---

# build-container-image

Build the Chronos container image from the repository-root `Dockerfile` and tag
it as `${repo}:${tag}`. By default the tag is `${version}-${ts}`, the build uses
`docker`, and the image is **not** pushed.

## Invocation

```
/build-container-image [--oci <tool>] [--repo <repo>] [--tag <tag>] [--push]
```

| Option | Required | Default | Description |
| --- | --- | --- | --- |
| `--oci <tool>` | No | `docker` | The OCI builder to use. **Only `docker` is supported for now**; reject any other value. |
| `--repo <repo>` | No | `ghcr.io/maple52046/chronos` | The image repository (the part before `:`). |
| `--tag <tag>` | No | *(empty)* | The image tag. If empty, use `${version}-${ts}`. |
| `--push` | No | *(off)* | Push the image after a successful build. Without it, **build only, no push**. |

## Quick start

Run the helper script with the parsed flags; it resolves `version`/`ts`, builds
the tag `${repo}:${tag}`, runs `docker build`, and (with `--push`) pushes:

```bash
skills/build-container-image/scripts/build-image.sh [--oci <tool>] [--repo <repo>] [--tag <tag>] [--push]
```

The script discovers the repository root from its own location, so it works
regardless of the current working directory. Pass the user's flags straight
through.

## What the script does

```
- [ ] 1. Parse flags; reject any --oci other than docker (exit 2)
- [ ] 2. Resolve version (Cargo.toml) and ts (UTC) when --tag is empty
- [ ] 3. Build the image reference: ${repo}:${tag}
- [ ] 4. Run docker build with the repository root as context
- [ ] 5. --push: docker push after a successful build
- [ ] 6. Print the final image reference and whether it was pushed
```

- `--oci`: only `docker` is accepted; any other value exits with an error.
- `version`: read from `workspace.package.version` in the root `Cargo.toml`
  (currently `1.0.0`); not hardcoded.
- `ts`: `date -u +%Y%m%d%H%M%S` (UTC), only used when `--tag` is empty.
- `tag`: `--tag` verbatim when given, else `${version}-${ts}`.
- Final reference example: `ghcr.io/maple52046/chronos:1.0.0-20260619234500`.

## Notes

- Pushing to `ghcr.io` requires the user to already be logged in
  (`docker login ghcr.io`). If the push fails with an auth error, stop and
  report it instead of retrying blindly.
- After the script finishes, report the final image reference (`${repo}:${tag}`)
  and whether it was pushed.

## Important reminders

- Default behavior minimizes side effects: **`docker` build only, no push**;
  `--push` is the only flag that performs a remote action.
- Only `docker` is supported right now; other `--oci` values are rejected.
