---
name: git-commit
description: >-
  Analyse the repository's changes, write a high-quality commit message, and run
  git commit (optionally git add and git push). Use when the user runs
  /git-commit or asks to commit staged changes with a generated message.
---

# git-commit

This is a thin Cursor wrapper. The canonical instructions live in
[`skills/git-commit/SKILL.md`](../../../skills/git-commit/SKILL.md)
(relative to the repository root). Read that file and follow it.

Invocation:

```
/git-commit [--auto-add] [--all] [--push] [--date <when>]
```
