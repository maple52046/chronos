---
name: build-container-image
description: >-
  Build the Chronos container image from the repository Dockerfile and tag it as
  `${repo}:${version}-${ts}`, optionally pushing afterwards. Use when the user
  runs /build-container-image or asks to build/push the Chronos container image.
---

# build-container-image

This is a thin Cursor wrapper. The canonical instructions live in
[`skills/build-container-image/SKILL.md`](../../../skills/build-container-image/SKILL.md)
(relative to the repository root). Read that file and follow it.

Invocation:

```
/build-container-image [--oci <tool>] [--repo <repo>] [--tag <tag>] [--push]
```
